#[path = "fixtures/resolution_revocation.rs"]
mod provider;
mod support;

use futures_util::FutureExt;
use hito_core::{
    Action, Actor, AdoptProviderRecording, InstallPlugin, OfferPluginService, PluginInstallation,
    ResolveSource, Services, SourceResolution, SourceResolutionOutcome,
};
use provider::{Event, Provider};
use serde_json::{Value, json};
use std::{panic::AssertUnwindSafe, time::Duration};
use tokio::task::JoinHandle;
use uuid::Uuid;

#[tokio::test]
async fn removing_a_connection_during_candidate_sources_stops_later_provider_requests() {
    for revoke in [false, true] {
        let (_directory, hito, actor, _, _) = support::library("resolution-revocation").await;
        let provider = Provider::start().await;
        let mut pending = None;
        let result = AssertUnwindSafe(exercise(&hito, &actor, &provider, revoke, &mut pending))
            .catch_unwind()
            .await;
        if let Some(pending) = pending {
            pending.abort();
            let _ = pending.await;
        }
        let stopped = provider.close().await;
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
        assert!(stopped, "the owned provider server must stop and join");
    }
}

async fn exercise(
    hito: &Services,
    actor: &Actor,
    provider: &Provider,
    revoke: bool,
    pending: &mut Option<JoinHandle<hito_core::Result<SourceResolution>>>,
) {
    let primary = connect(hito, actor, provider.metadata_url(), None).await;
    let audio = connect(
        hito,
        actor,
        provider.audio_url(),
        Some("owned-resolution-private-token".into()),
    )
    .await;
    let recording = hito
        .catalog
        .adopt_provider(
            actor,
            &primary.id,
            AdoptProviderRecording {
                installation_revision: primary.revision,
                provider_recording_id: "owned/primary".into(),
            },
        )
        .await
        .unwrap();
    let recording_id = Uuid::parse_str(&recording.id).unwrap();
    let playlist = Uuid::new_v4();
    support::synchronize(
        hito,
        actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "Owned personal title".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 400,
                speed: 1.25,
            },
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Owned playlist".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 1,
            },
        ],
    )
    .await;
    let playlist = playlist.to_string();
    let before = snapshot(hito, actor, &recording.id, &playlist).await;
    assert!(
        hito.catalog
            .sources(actor, &recording.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(provider.events().await.is_empty());
    *pending = Some({
        let catalog = hito.catalog.clone();
        let actor = actor.clone();
        let recording = recording.id.clone();
        tokio::spawn(async move {
            catalog
                .resolve_source_until(
                    &actor,
                    &recording,
                    ResolveSource {
                        request_id: Uuid::new_v4(),
                    },
                    std::future::pending(),
                )
                .await
        })
    });
    tokio::time::timeout(Duration::from_secs(3), provider.candidate_sources_entered())
        .await
        .expect("search and first candidate sources must reach the owned barrier");
    assert_eq!(
        provider.events().await,
        [Event::Search, Event::DetailA, Event::SourcesA]
    );
    if revoke {
        hito.plugins
            .remove(actor, &audio.id, audio.revision)
            .await
            .unwrap();
    }
    provider.release_candidate_sources();
    let settled = tokio::time::timeout(Duration::from_secs(3), pending.as_mut().unwrap())
        .await
        .expect("source resolution must settle after the owned barrier opens");
    pending.take();
    let resolution = settled.expect("the source resolver must join").unwrap();
    assert!(
        provider.authenticated(),
        "provider requests must use the approved credential"
    );
    assert!(resolution.recording_id == recording.id);
    if revoke {
        assert!(matches!(
            resolution.outcome,
            SourceResolutionOutcome::NoSuitableSource
        ));
        assert!(
            resolution.sources.is_empty(),
            "revocation must prevent source publication"
        );
        assert!(
            snapshot(hito, actor, &recording.id, &playlist).await == before,
            "revocation must preserve primary metadata, private edits, identity and collections"
        );
        assert_eq!(
            provider.events().await,
            [Event::Search, Event::DetailA, Event::SourcesA],
            "revocation must stop later provider requests after the held exchange settles"
        );
    } else {
        assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
        assert_eq!(resolution.sources.len(), 1);
        assert!(resolution.sources[0].ready);
        assert_eq!(
            provider.events().await,
            [
                Event::Search,
                Event::DetailA,
                Event::SourcesA,
                Event::DetailB,
                Event::SourcesB
            ],
            "the active control must reject unavailable A and reach eligible B"
        );
    }
}

async fn connect(
    hito: &Services,
    actor: &Actor,
    base_url: String,
    bearer_token: Option<String>,
) -> PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned resolution fixture".into(),
                base_url,
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token,
            },
        )
        .await
        .unwrap()
}

async fn snapshot(hito: &Services, actor: &Actor, recording: &str, playlist: &str) -> Value {
    json!({
        "recordings": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(actor, recording, Default::default()).await.unwrap(),
        "playlist": hito.collection.playlist(actor, playlist).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
    })
}

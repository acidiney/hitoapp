#[path = "fixtures/resolution_revocation.rs"]
mod provider;
#[path = "fixtures/resolution_publication.rs"]
mod publication;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    Action, Actor, AdoptProviderRecording, Error, InstallPlugin, OfferPluginService, OpenedAudio,
    PluginInstallation, ResolveSource, Services, SourceResolution, SourceResolutionOutcome,
};
use provider::{Event, Provider};
use publication::Publication;
use serde_json::{Value, json};
use std::{panic::AssertUnwindSafe, time::Duration};
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;
use uuid::Uuid;

#[derive(Clone, Copy)]
enum AfterCollision {
    Continue,
    Cancel,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn a_binding_claimed_after_preparation_does_not_block_a_later_eligible_audio_source() {
    run(AfterCollision::Continue).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn cancelling_a_rejected_publication_prevents_later_candidate_requests_and_replays_without_io()
 {
    run(AfterCollision::Cancel).await;
}

async fn run(after_collision: AfterCollision) {
    let (directory, hito, actor, _, _) = support::library("resolution-binding-race").await;
    let provider = Provider::start().await;
    let publication = Publication::default();
    let mut pending = None;
    let result = AssertUnwindSafe(exercise(
        &directory,
        &hito,
        &actor,
        &provider,
        &publication,
        &mut pending,
        after_collision,
    ))
    .catch_unwind()
    .await;
    publication.release();
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

async fn exercise(
    directory: &tempfile::TempDir,
    hito: &Services,
    actor: &Actor,
    provider: &Provider,
    publication: &Publication,
    pending: &mut Option<JoinHandle<hito_core::Result<SourceResolution>>>,
    after_collision: AfterCollision,
) {
    let first_audio = support::write_wav(&directory.path().join("candidate-a.wav"), 100);
    let second_audio = support::write_wav(&directory.path().join("candidate-b.wav"), 200);
    assert!(
        first_audio != second_audio,
        "the owned audio fixtures must be distinct"
    );
    provider
        .ready_audio(first_audio.clone(), second_audio.clone())
        .await;
    let primary = connect(hito, actor, provider.metadata_url(), None).await;
    let audio = connect(
        hito,
        actor,
        provider.audio_url(),
        Some("owned-resolution-private-token".into()),
    )
    .await;
    let target = adopt(hito, actor, &primary, "owned/primary").await;
    let recording_id = Uuid::parse_str(&target.id).unwrap();
    let playlist_id = Uuid::new_v4();
    support::synchronize(
        hito,
        actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "Owned personal title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "Owned personal artist".into(),
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
                playlist_id,
                name: "Owned playlist".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 1,
            },
        ],
    )
    .await;
    let playlist_id = playlist_id.to_string();
    let original_primary = primary_state(hito, actor, &target.id).await;
    assert!(
        hito.catalog
            .sources(actor, &target.id)
            .await
            .unwrap()
            .is_empty()
    );
    let request_id = Uuid::new_v4();
    let (cancel, cancelled) = oneshot::channel();
    let started = tokio::time::Instant::now();
    let subscriber = tracing_subscriber::registry().with(publication.clone());
    *pending = Some({
        let catalog = hito.catalog.clone();
        let actor = actor.clone();
        let recording = target.id.clone();
        tokio::spawn(
            async move {
                catalog
                    .resolve_source_until(&actor, &recording, ResolveSource { request_id }, async {
                        let _ = cancelled.await;
                    })
                    .await
            }
            .with_subscriber(subscriber),
        )
    });
    tokio::time::timeout(Duration::from_secs(3), publication.entered())
        .await
        .expect("the selected candidate must reach the static publication boundary");
    assert_eq!(
        provider.events().await,
        [Event::Search, Event::DetailA, Event::SourcesA],
        "candidate A must be prepared before another operation can claim it"
    );
    assert_eq!(
        hito.catalog
            .recordings(actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
    let adoption = tokio::time::timeout(
        Duration::from_secs(2),
        adopt(hito, actor, &audio, "owned/candidate-a"),
    )
    .await;
    let occupied = match adoption {
        Ok(occupied) => occupied,
        Err(_) => {
            let events = provider.events().await;
            let expired = publication.expired();
            panic!(
                "owned_adoption_timeout stage=adoption_after_publication_marker events={events:?} publication_expired={expired}"
            );
        }
    };
    assert!(
        target.id != occupied.id,
        "the racing adoption must own a separate recording"
    );
    assert!(
        primary_state(hito, actor, &target.id).await == original_primary,
        "the racing adoption must not change the target's primary or private metadata"
    );
    let occupied_source = hito
        .catalog
        .sources(actor, &occupied.id)
        .await
        .unwrap()
        .remove(0);
    assert!(occupied_source.ready);
    assert!(
        audio_bytes(hito, actor, &occupied_source.id).await == first_audio,
        "candidate A must play for its racing owner before publication resumes"
    );
    let before = stable_state(hito, actor, &target.id, &occupied.id, &playlist_id).await;
    assert!(
        !publication.expired(),
        "the publication boundary must remain held through the racing adoption"
    );
    if matches!(after_collision, AfterCollision::Cancel) {
        cancel
            .send(())
            .expect("the owned cancellation must reach the held resolver");
    }
    publication.release();
    let settled = tokio::time::timeout(Duration::from_secs(3), pending.as_mut().unwrap())
        .await
        .expect("automatic source resolution must settle within the owned test budget");
    pending.take();
    let resolved = settled.expect("the automatic source resolver must join");
    assert!(
        !publication.expired(),
        "the owned publication barrier must not time out"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "continuation must remain within the original resolution budget"
    );
    assert!(
        stable_state(hito, actor, &target.id, &occupied.id, &playlist_id).await == before,
        "automatic resolution must preserve existing identities, primary metadata, private edits and collections"
    );
    assert!(
        audio_bytes(hito, actor, &occupied_source.id).await == first_audio,
        "the occupied source must retain its original owner and audio bytes"
    );
    let resolution = match resolved {
        Ok(resolution) => resolution,
        Err(Error::Conflict(_)) => {
            panic!(
                "a candidate claimed after preparation must not block a later eligible candidate"
            )
        }
        Err(_) => panic!("the owned resolution must not fail with another error"),
    };
    if matches!(after_collision, AfterCollision::Cancel) {
        assert!(
            matches!(resolution.outcome, SourceResolutionOutcome::Cancelled),
            "a rejected publication must settle cancellation before continuing provider work"
        );
        assert_eq!(resolution.request_id, request_id);
        assert!(resolution.recording_id == target.id);
        assert!(resolution.sources.is_empty());
        assert!(
            hito.catalog
                .sources(actor, &target.id)
                .await
                .unwrap()
                .is_empty()
        );
        let events = provider.events().await;
        assert_eq!(
            events,
            [
                Event::Search,
                Event::DetailA,
                Event::SourcesA,
                Event::DetailA,
                Event::SourcesA,
                Event::AudioA,
                Event::AudioA
            ],
            "cancellation after rejected publication must prevent B detail, sources and audio"
        );
        let replay = tokio::time::timeout(
            Duration::from_secs(3),
            hito.catalog.resolve_source_until(
                actor,
                &target.id,
                ResolveSource { request_id },
                std::future::pending(),
            ),
        )
        .await
        .expect("the cancelled receipt must replay without waiting for a new lease")
        .unwrap();
        assert_eq!(replay.resolution_id, resolution.resolution_id);
        assert_eq!(replay.request_id, request_id);
        assert!(replay.recording_id == target.id);
        assert!(matches!(replay.outcome, SourceResolutionOutcome::Cancelled));
        assert!(replay.sources.is_empty());
        assert_eq!(
            provider.events().await,
            events,
            "cancelled receipt replay must not repeat provider IO"
        );
        assert!(
            stable_state(hito, actor, &target.id, &occupied.id, &playlist_id).await == before,
            "cancelled receipt replay must preserve both recordings and all private state"
        );
        assert!(
            provider.authenticated(),
            "audio provider requests must use the approved credential"
        );
        return;
    }
    assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(resolution.request_id, request_id);
    assert!(resolution.recording_id == target.id);
    assert_eq!(resolution.sources.len(), 1);
    let selected = &resolution.sources[0];
    assert!(selected.ready && selected.recording_id == target.id);
    assert!(
        selected.id != occupied_source.id,
        "candidate A's existing source must not move to the target"
    );
    assert!(
        audio_bytes(hito, actor, &selected.id).await == second_audio,
        "the target must actually play candidate B's distinct original bytes"
    );
    let events = provider.events().await;
    for (event, expected) in [
        (Event::Search, 1),
        (Event::DetailA, 2),
        (Event::SourcesA, 2),
        (Event::DetailB, 1),
        (Event::SourcesB, 1),
        (Event::AudioA, 2),
        (Event::AudioB, 1),
    ] {
        assert_eq!(
            events.iter().filter(|observed| **observed == event).count(),
            expected,
            "the owned protocol must prove candidate B was resolved and each source kept its original bytes"
        );
    }
    assert!(
        provider.authenticated(),
        "audio provider requests must use the approved credential"
    );
}

async fn primary_state(hito: &Services, actor: &Actor, recording: &str) -> Value {
    let metadata = hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap();
    json!({"selected": metadata.selected_observation, "title": metadata.title, "artist": metadata.artist})
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
                name: "Owned bound-candidate fixture".into(),
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

async fn adopt(
    hito: &Services,
    actor: &Actor,
    installation: &PluginInstallation,
    provider_recording_id: &str,
) -> hito_core::Recording {
    hito.catalog
        .adopt_provider(
            actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: provider_recording_id.into(),
            },
        )
        .await
        .unwrap()
}

async fn audio_bytes(hito: &Services, actor: &Actor, source: &str) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let OpenedAudio::Content(mut audio) = hito
            .playback
            .open_audio(actor, source, Default::default())
            .await
            .unwrap()
        else {
            panic!("an owned ready source must open its original audio");
        };
        assert!(audio.content_type == "audio/wav");
        let mut bytes = Vec::new();
        while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
            bytes.extend_from_slice(&chunk);
        }
        bytes
    })
    .await
    .expect("the owned audio body must finish within the test budget")
}

async fn stable_state(
    hito: &Services,
    actor: &Actor,
    target: &str,
    occupied: &str,
    playlist: &str,
) -> Value {
    let target_metadata = hito
        .catalog
        .recording_metadata(actor, target, Default::default())
        .await
        .unwrap();
    json!({
        "recordings": without_availability(serde_json::to_value(hito.catalog.recordings(actor, Default::default()).await.unwrap()).unwrap(), "items"),
        "target_primary": target_metadata.selected_observation,
        "target_title": target_metadata.title,
        "target_artist": target_metadata.artist,
        "occupied_metadata": hito.catalog.recording_metadata(actor, occupied, Default::default()).await.unwrap(),
        "occupied_sources": hito.catalog.sources(actor, occupied).await.unwrap(),
        "playlist": without_availability(serde_json::to_value(hito.collection.playlist(actor, playlist).await.unwrap()).unwrap(), "entries"),
        "activity": without_availability(serde_json::to_value(hito.collection.activity(actor).await.unwrap()).unwrap(), "history"),
    })
}

fn without_availability(mut page: Value, field: &str) -> Value {
    for item in page[field].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("available");
    }
    page
}

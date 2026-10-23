use hito_core::test_support::artist_catalog_profiles_service as catalog_provider;
#[path = "fixtures/artist_catalog_profiles_library.rs"]
#[allow(dead_code)]
mod library;
#[path = "fixtures/artist_catalog_playback_provider.rs"]
mod playback_provider;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    AdoptProviderRecording, ArtistProfileAvailability, AudioRequest, InstallPlugin,
    OfferPluginService, OpenedAudio,
};
use serde_json::Value;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::{task::JoinSet, time::timeout};

struct Observed {
    refresh: Result<hito_core::Result<bool>, tokio::time::error::Elapsed>,
    profile: hito_core::Result<ArtistProfileAvailability>,
    profile_was_held: bool,
    audio: Vec<u8>,
    before: Value,
    after: Value,
    sources_before: Value,
    sources_after: Value,
}

#[tokio::test]
async fn a_slow_artist_profile_does_not_block_same_account_playback_source_recovery() {
    let library = library::Library::prepare().await;
    let audio_directory = tempfile::tempdir().unwrap();
    let original = support::write_wav(&audio_directory.path().join("owned.wav"), 37);
    let mut catalog = catalog_provider::CatalogServer::start().await;
    let mut playback = playback_provider::PlaybackServer::start(original.clone()).await;
    let mut reads = JoinSet::new();
    let result = AssertUnwindSafe(exercise(&library, &catalog, &playback, &mut reads))
        .catch_unwind()
        .await;
    catalog.release_profile().await;
    reads.shutdown().await;
    let catalog_joined = catalog.finish().await;
    let playback_joined = playback.finish().await;
    let observed = match result {
        Ok(observed) => observed,
        Err(panic) => resume_unwind(panic),
    };
    assert!(
        catalog_joined,
        "The owned catalog server must join shutdown"
    );
    assert!(
        playback_joined,
        "The owned playback server must join shutdown"
    );
    assert!(
        observed.profile_was_held,
        "Playback must finish while the artist profile is still held"
    );
    assert!(
        matches!(observed.refresh, Ok(Ok(true))),
        "A pending artist profile must leave unrelated same-account playback source recovery available: {:?}",
        observed.refresh
    );
    let Ok(ArtistProfileAvailability::Available { profile, .. }) = observed.profile else {
        panic!("The released artist profile must finish successfully");
    };
    assert!(profile == catalog_provider::expected_profile());
    assert_eq!(observed.audio, original);
    assert!(
        observed.before == observed.after,
        "Metadata and saved collection state must be preserved"
    );
    assert!(
        observed.sources_before == observed.sources_after,
        "Refreshing an unchanged ready source must preserve its public identity"
    );
    assert_eq!(
        catalog.counts().await,
        catalog_provider::Counts {
            manifests: 1,
            descriptors: 1,
            profiles: 1,
            unexpected: 0,
        }
    );
    assert_eq!(
        playback.counts(),
        playback_provider::Counts {
            manifests: 1,
            details: 1,
            sources: 3,
            audio: 1,
            unexpected: 0,
        }
    );
}

async fn exercise(
    library: &library::Library,
    catalog_provider: &catalog_provider::CatalogServer,
    playback: &playback_provider::PlaybackServer,
    reads: &mut JoinSet<hito_core::Result<ArtistProfileAvailability>>,
) -> Observed {
    let hito = &library.hito;
    let actor = &library.actor;
    let service = hito
        .plugins
        .offer_service(
            &library.admin,
            OfferPluginService {
                name: "Owned playback provider".into(),
                base_url: playback.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let recording = hito
        .catalog
        .adopt_provider(
            actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: playback_provider::RECORDING_ID.into(),
            },
        )
        .await
        .unwrap();
    assert!(
        hito.catalog
            .refresh_playback_sources_until(actor, &recording.id, std::future::pending())
            .await
            .unwrap()
    );
    let source = hito
        .catalog
        .sources(actor, &recording.id)
        .await
        .unwrap()
        .remove(0);
    assert!(source.ready);
    let association = library
        .review_artist(&catalog_provider.url, catalog_provider::ARTIST_ID)
        .await;
    let before = library.snapshot().await;
    let sources_before =
        serde_json::to_value(hito.catalog.sources(actor, &recording.id).await.unwrap()).unwrap();
    let gate = catalog_provider.hold_profile().await;
    let request_catalog = hito.catalog.clone();
    let request_actor = actor.clone();
    let reviewed_recording = library.recording_id.clone();
    reads.spawn(async move {
        request_catalog
            .recording_artist_profile_until(
                &request_actor,
                &reviewed_recording,
                &association.id,
                std::future::pending(),
            )
            .await
    });
    timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("The owned profile must reach its provider before playback is requested");
    let refresh = timeout(
        Duration::from_secs(3),
        hito.catalog
            .refresh_playback_sources_until(actor, &recording.id, std::future::pending()),
    )
    .await;
    let audio = timeout(Duration::from_secs(3), async {
        let opened = hito
            .playback
            .open_audio(actor, &source.id, AudioRequest::default())
            .await
            .unwrap();
        let OpenedAudio::Content(mut content) = opened else {
            panic!("The owned ready WAV must open");
        };
        assert_eq!(content.content_type, "audio/wav");
        let mut bytes = Vec::new();
        let mut body = content.body.take().unwrap();
        while let Some(chunk) = body.chunk().await.unwrap() {
            assert!(chunk.len() <= 32_768_usize.saturating_sub(bytes.len()));
            bytes.extend_from_slice(&chunk);
        }
        bytes
    })
    .await
    .expect("Owned audio must finish while the catalog profile remains held");
    let early_profile = reads.try_join_next();
    let profile_was_held = early_profile.is_none();
    catalog_provider.release_profile().await;
    let profile = match early_profile {
        Some(result) => result.expect("The owned artist profile task must not panic"),
        None => timeout(Duration::from_secs(3), reads.join_next())
            .await
            .expect("The released profile must finish promptly")
            .expect("The profile task must remain owned until joined")
            .expect("The owned artist profile task must not panic"),
    };
    Observed {
        refresh,
        profile,
        profile_was_held,
        audio,
        before,
        after: library.snapshot().await,
        sources_before,
        sources_after: serde_json::to_value(
            hito.catalog.sources(actor, &recording.id).await.unwrap(),
        )
        .unwrap(),
    }
}

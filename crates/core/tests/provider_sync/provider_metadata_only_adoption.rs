use hito_core::test_support::provider_attachment as fixture;
use hito_core::test_support::support;

use hito_core::{
    AdoptProviderRecording, Error, ProviderCandidate, Recording, ResolveSource,
    SourceResolutionOutcome,
};
use serde_json::json;
use uuid::Uuid;

const DETAIL: &str = "item/metadata-only";

fn detail() -> serde_json::Value {
    json!({
        "id": DETAIL, "title": "Owned tune", "artist": "Owned performer",
        "duration_ms": 1000, "revision": "meta-r1",
        "release": {"id": "album/owned", "title": "Owned album", "artist": "Owned performer"},
        "track_number": 1
    })
}

async fn adopt(
    hito: &hito_core::Services,
    actor: &hito_core::Actor,
    installation: &hito_core::PluginInstallation,
) -> Result<Recording, Error> {
    hito.catalog
        .adopt_provider(
            actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: DETAIL.into(),
            },
        )
        .await
}

#[tokio::test]
async fn a_provider_without_sources_saves_a_recording_with_metadata_and_no_sources() {
    let meta = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _library, _music) =
        support::library("metadata-only-adoption").await;
    let installation = fixture::connect(
        &hito,
        &actor,
        &meta,
        "org.hito.metadata-only-fixture",
        &["search", "metadata", "artwork"],
    )
    .await;
    meta.reply("v1/recordings/detail", detail()).await;
    let saved = adopt(&hito, &actor, &installation)
        .await
        .expect("a provider that cannot supply audio can still contribute a saved recording");
    assert_eq!(saved.title, "Owned tune");
    assert_eq!(saved.artist, "Owned performer");
    assert_eq!(saved.album.as_deref(), Some("Owned album"));
    assert!(!saved.available);
    assert!(
        hito.catalog
            .sources(&actor, &saved.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(meta.requests("v1/sources").await.is_empty());
    let metadata = hito
        .catalog
        .recording_metadata(&actor, &saved.id, Default::default())
        .await
        .unwrap();
    let observed = metadata.selected_observation.unwrap();
    assert_eq!(observed.provider, "org.hito.metadata-only-fixture");
    assert_eq!(observed.provider_recording_id.as_deref(), Some(DETAIL));
    assert_eq!(
        observed.plugin_installation_id.as_deref(),
        Some(installation.id.as_str())
    );
    let again = adopt(&hito, &actor, &installation).await.unwrap();
    assert_eq!(again.id, saved.id);
    assert!(meta.requests("v1/sources").await.is_empty());
    assert!(
        hito.catalog
            .sources(&actor, &saved.id)
            .await
            .unwrap()
            .is_empty()
    );
    let candidate = ProviderCandidate {
        installation_id: installation.id.clone(),
        installation_revision: installation.revision,
        provider_recording_id: DETAIL.into(),
    };
    assert!(matches!(
        hito.catalog
            .review_provider_source(&actor, &saved.id, candidate)
            .await,
        Err(Error::Invalid(_))
    ));
    meta.shutdown().await;
}

#[tokio::test]
async fn a_saved_recording_without_provider_sources_resolves_audio_through_another_provider() {
    let meta = fixture::ProviderServer::start().await;
    let audio = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _library, _music) =
        support::library("metadata-only-resolution").await;
    let installation = fixture::connect(
        &hito,
        &actor,
        &meta,
        "org.hito.metadata-only-fixture",
        &["search", "metadata", "artwork"],
    )
    .await;
    meta.reply("v1/recordings/detail", detail()).await;
    let saved = adopt(&hito, &actor, &installation).await.unwrap();

    let none = hito
        .catalog
        .resolve_source_until(
            &actor,
            &saved.id,
            ResolveSource {
                request_id: Uuid::new_v4(),
            },
            std::future::pending(),
        )
        .await
        .unwrap();
    assert!(matches!(
        none.outcome,
        SourceResolutionOutcome::NoSuitableSource
    ));
    assert!(none.sources.is_empty());

    fixture::connect(
        &hito,
        &actor,
        &audio,
        "org.hito.audio-fixture",
        &["search", "sources", "metadata"],
    )
    .await;
    let found = json!({
        "id": "audio/owned", "title": "Owned performer - Owned tune (Official Audio)",
        "artist": "Owned performer", "duration_ms": 1000,
        "release": null, "disc_number": null, "track_number": null, "revision": "audio-r1"
    });
    audio
        .reply("v1/search", json!({"items": [found], "next_cursor": null}))
        .await;
    audio.reply("v1/recordings/detail", found.clone()).await;
    audio
        .reply(
            "v1/sources",
            json!({"items": [{"id": "audio/owned-source", "label": "Owned audio", "ready": true}]}),
        )
        .await;
    audio
        .reply(
            "v1/recordings/metadata",
            json!({
                "recording": found, "musical_title": "Owned tune", "album_title": "Owned album",
                "source_ids": ["audio/owned-source"]
            }),
        )
        .await;
    let resolved = hito
        .catalog
        .resolve_source_until(
            &actor,
            &saved.id,
            ResolveSource {
                request_id: Uuid::new_v4(),
            },
            std::future::pending(),
        )
        .await
        .unwrap();
    assert!(matches!(resolved.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(resolved.sources.len(), 1);
    assert_eq!(resolved.sources[0].recording_id, saved.id);
    for path in ["v1/search", "v1/sources", "v1/audio"] {
        assert!(meta.requests(path).await.is_empty(), "{path}");
    }
    meta.shutdown().await;
    audio.shutdown().await;
}

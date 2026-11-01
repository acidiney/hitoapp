use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    AdoptProviderRecording, ChooseSource, ImportProviderPlaylist, ResetSourceChoice, ResolveSource,
    SourceResolutionOutcome,
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn a_playlist_copy_plays_through_the_sources_of_the_provider_that_listed_it() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("playlist-own-sources").await;
    let installed =
        fixture::connect(&hito, &actor, &server, &["playlists", "search", "sources"]).await;
    let recording = json!({
        "id": "recording/own", "title": "Own tune", "artist": "Own performer",
        "duration_ms": 90_000, "revision": "own-r1"
    });
    fixture::playlist(
        &server,
        "playlist/own",
        "snapshot-one",
        vec![json!({"occurrence_id": "first", "item": {"kind": "recording", "recording": recording}})],
    )
    .await;
    server
        .reply(
            "v1/search",
            json!({"items": [recording], "next_cursor": null}),
        )
        .await;
    server
        .reply("v1/recordings/detail", recording.clone())
        .await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{"id": "audio/own", "label": "Original", "ready": true}]}),
        )
        .await;

    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "own-playlist".into(),
            },
        )
        .await
        .unwrap();
    let detail = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    let imported = detail.entries[0].recording_id.clone().unwrap().to_string();
    assert!(
        hito.catalog
            .sources(&actor, &imported)
            .await
            .unwrap()
            .is_empty()
    );

    let resolved = hito
        .catalog
        .resolve_source_until(
            &actor,
            &imported,
            ResolveSource {
                request_id: Uuid::new_v4(),
            },
            std::future::pending(),
        )
        .await
        .unwrap();
    assert!(matches!(resolved.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(resolved.sources.len(), 1);
    assert!(resolved.sources[0].ready);
    let again = hito.catalog.sources(&actor, &imported).await.unwrap();
    assert_eq!(again.len(), 1);
    let refreshed = hito
        .catalog
        .refresh_sources(&actor, &imported)
        .await
        .unwrap();
    assert_eq!(refreshed.len(), 1);
}

async fn copy_with_one_tune(
    name: &str,
) -> (
    tempfile::TempDir,
    hito_core::Services,
    hito_core::Actor,
    fixture::ProviderServer,
    String,
) {
    let server = fixture::ProviderServer::start().await;
    let (dir, hito, actor, _shared, _music) = support::library(name).await;
    let installed =
        fixture::connect(&hito, &actor, &server, &["playlists", "search", "sources"]).await;
    let recording = json!({
        "id": "recording/own", "title": "Own tune", "artist": "Own performer",
        "duration_ms": 90_000, "revision": "own-r1"
    });
    fixture::playlist(
        &server,
        "playlist/own",
        "snapshot-one",
        vec![json!({"occurrence_id": "first", "item": {"kind": "recording", "recording": recording}})],
    )
    .await;
    server
        .reply(
            "v1/search",
            json!({"items": [recording], "next_cursor": null}),
        )
        .await;
    server
        .reply("v1/recordings/detail", recording.clone())
        .await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{"id": "audio/own", "label": "Original", "ready": true}]}),
        )
        .await;
    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "own-playlist".into(),
            },
        )
        .await
        .unwrap();
    let detail = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    let imported = detail.entries[0].recording_id.clone().unwrap().to_string();
    (dir, hito, actor, server, imported)
}

async fn resolve(
    hito: &hito_core::Services,
    actor: &hito_core::Actor,
    recording: &str,
) -> hito_core::SourceResolution {
    hito.catalog
        .resolve_source_until(
            actor,
            recording,
            ResolveSource {
                request_id: Uuid::new_v4(),
            },
            std::future::pending(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn a_listener_choice_stays_protected_and_resetting_it_keeps_the_primary_binding() {
    let (_dir, hito, actor, server, imported) = copy_with_one_tune("own-sources-choice").await;
    let first = resolve(&hito, &actor, &imported).await;
    assert!(matches!(first.outcome, SourceResolutionOutcome::Ready));
    let source = hito
        .catalog
        .sources(&actor, &imported)
        .await
        .unwrap()
        .remove(0);
    let chosen = hito
        .catalog
        .choose_source(
            &actor,
            &imported,
            ChooseSource {
                request_id: Uuid::new_v4(),
                source_id: Uuid::parse_str(&source.id).unwrap(),
            },
        )
        .await
        .unwrap();
    let before = server.requests("v1/sources").await.len();
    let again = resolve(&hito, &actor, &imported).await;
    assert!(matches!(
        again.outcome,
        SourceResolutionOutcome::ProtectedChoice
    ));
    assert_eq!(server.requests("v1/sources").await.len(), before);
    assert_eq!(
        hito.catalog.sources(&actor, &imported).await.unwrap().len(),
        1
    );

    hito.catalog
        .reset_source_choice(
            &actor,
            &imported,
            ResetSourceChoice {
                request_id: Uuid::new_v4(),
                expected_revision: chosen.revision,
            },
        )
        .await
        .unwrap();
    let kept = hito.catalog.sources(&actor, &imported).await.unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].id, source.id);
    assert!(kept[0].ready);
}

#[tokio::test]
async fn a_candidate_that_belongs_to_another_recording_is_left_alone() {
    let (_dir, hito, actor, server, imported) = copy_with_one_tune("own-sources-occupied").await;
    let installed = hito
        .plugins
        .settings(&actor)
        .await
        .unwrap()
        .installations
        .remove(0);
    let other = json!({
        "id": "recording/other", "title": "Own tune", "artist": "Own performer",
        "duration_ms": 90_000, "revision": "other-r1"
    });
    server.reply("v1/recordings/detail", other.clone()).await;
    let adopted = hito
        .catalog
        .adopt_provider(
            &actor,
            &installed.id,
            AdoptProviderRecording {
                installation_revision: installed.revision,
                provider_recording_id: "recording/other".into(),
            },
        )
        .await
        .unwrap();
    assert_ne!(adopted.id, imported);
    server
        .reply("v1/search", json!({"items": [other], "next_cursor": null}))
        .await;
    let outcome = resolve(&hito, &actor, &imported).await;
    assert!(matches!(
        outcome.outcome,
        SourceResolutionOutcome::NoSuitableSource
    ));
    assert!(
        hito.catalog
            .sources(&actor, &imported)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        hito.catalog
            .sources(&actor, &adopted.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{ImportProviderPlaylist, ResolveSource, SourceResolutionOutcome};
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

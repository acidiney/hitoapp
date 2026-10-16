#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use hito_core::{ImportProviderPlaylist, InstallPlugin, OfferPluginService};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn a_playlist_copy_retains_ordered_duplicate_occurrences_without_creating_audio() {
    let server = fixture::ProviderServer::start().await;
    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.playlist-fixture",
                "name": "Playlist fixture",
                "protocol_version": 1,
                "capabilities": ["playlists"]
            }),
        )
        .await;
    server
        .reply(
            "v1/playlists/detail",
            json!({
                "id": "playlist/one",
                "name": "Provider selection",
                "snapshot": "snapshot-one",
                "total_count": 3,
                "attribution": {"label": "Playlist fixture"}
            }),
        )
        .await;
    let first = json!({
        "kind": "recording",
        "recording": {
            "id": "recording/first",
            "title": "First recording",
            "artist": "Provider artist",
            "duration_ms": 123_000,
            "revision": "recording-one"
        }
    });
    let second = json!({
        "kind": "recording",
        "recording": {
            "id": "recording/second",
            "title": "Second recording",
            "artist": "Provider artist"
        }
    });
    server
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": "playlist/one",
                "snapshot": "snapshot-one",
                "items": [
                    {"occurrence_id": "first-occurrence", "item": first},
                    {"occurrence_id": "second-occurrence", "item": second},
                    {"occurrence_id": "third-occurrence", "item": first}
                ],
                "next_cursor": null
            }),
        )
        .await;
    let (_dir, hito, actor, _shared, _music) = support::library("playlist-import").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let request_id = Uuid::new_v4();
    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id,
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/playlists/supplied-link".into(),
            },
        )
        .await
        .unwrap();
    let detail = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();

    assert_eq!(detail.playlist.kind, "local");
    assert_eq!(detail.playlist.name, "Provider selection");
    assert_eq!(copied.request_id, request_id);
    assert_eq!(detail.entries.len(), 3);
    assert_eq!(
        detail
            .entries
            .iter()
            .map(|entry| (entry.position, entry.title.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (0, "First recording"),
            (1, "Second recording"),
            (2, "First recording")
        ]
    );
    let entry_ids: std::collections::HashSet<_> = detail
        .entries
        .iter()
        .map(|entry| Uuid::parse_str(&entry.entry_id).unwrap())
        .collect();
    assert_eq!(entry_ids.len(), 3);
    assert_eq!(
        detail.entries[0].recording_id,
        detail.entries[2].recording_id
    );
    assert_ne!(
        detail.entries[0].recording_id,
        detail.entries[1].recording_id
    );
    assert!(detail.entries.iter().all(|entry| !entry.available));
    let recordings = hito
        .catalog
        .recordings(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 2);
    for recording in recordings.items {
        assert!(!recording.available);
        assert!(
            hito.catalog
                .sources(&actor, &recording.id)
                .await
                .unwrap()
                .is_empty()
        );
    }
}

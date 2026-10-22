use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;

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
                accept_manifest_digest: None,
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

#[tokio::test]
async fn unavailable_and_unknown_occurrences_retain_display_and_attribution_after_uninstall() {
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
                "id": "playlist/retained",
                "name": "Retained selection",
                "snapshot": "retained-one",
                "total_count": 4,
                "attribution": {"label": "Curated by the fixture owner"}
            }),
        )
        .await;
    let known = json!({
        "kind": "recording",
        "recording": {"id": "recording/known", "title": "Known recording", "artist": "Known artist"}
    });
    server
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": "playlist/retained",
                "snapshot": "retained-one",
                "items": [
                    {"occurrence_id": "known-first", "item": known},
                    {"occurrence_id": "unavailable", "item": {
                        "kind": "unavailable", "provider_recording_id": "recording/withdrawn",
                        "title": "Withdrawn recording", "artist": "Retained artist"
                    }},
                    {"occurrence_id": "unknown", "item": {"kind": "unknown"}},
                    {"occurrence_id": "known-again", "item": known}
                ],
                "next_cursor": null
            }),
        )
        .await;
    let (_dir, hito, actor, _shared, _music) = support::library("playlist-retained").await;
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
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/retained-selection".into(),
            },
        )
        .await
        .unwrap();
    let before = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    hito.plugins
        .remove(&actor, &installed.id, installed.revision)
        .await
        .unwrap();
    let after = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    let before = serde_json::to_value(before).unwrap();
    let after = serde_json::to_value(after).unwrap();
    assert_eq!(after, before);
    assert_eq!(after["origin"]["provider"], "org.hito.playlist-fixture");
    assert_eq!(after["origin"]["installation_id"], installed.id);
    assert_eq!(after["origin"]["provider_playlist_id"], "playlist/retained");
    assert_eq!(after["origin"]["snapshot"], "retained-one");
    assert_eq!(
        after["origin"]["attribution"]["label"],
        "Curated by the fixture owner"
    );
    let entries = after["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 4);
    let ids = entries
        .iter()
        .map(|entry| Uuid::parse_str(entry["entry_id"].as_str().unwrap()).unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 4);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["position"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(entries[0]["recording_id"], entries[3]["recording_id"]);
    assert!(Uuid::parse_str(entries[0]["recording_id"].as_str().unwrap()).is_ok());
    assert_eq!(entries[0]["origin"]["kind"], "recording");
    assert_eq!(
        entries[0]["origin"]["provider_recording_id"],
        "recording/known"
    );
    assert!(entries[1]["recording_id"].is_null());
    assert_eq!(entries[1]["title"], "Withdrawn recording");
    assert_eq!(entries[1]["artist"], "Retained artist");
    assert_eq!(entries[1]["origin"]["kind"], "unavailable");
    assert_eq!(
        entries[1]["origin"]["provider_recording_id"],
        "recording/withdrawn"
    );
    assert!(entries[2]["recording_id"].is_null());
    assert_eq!(entries[2]["title"], "Unknown provider item");
    assert_eq!(entries[2]["origin"]["kind"], "unknown");
    assert!(entries[2]["origin"]["provider_recording_id"].is_null());
    assert_eq!(entries[3]["origin"]["occurrence_id"], "known-again");
    assert!(entries.iter().all(|entry| entry["available"] == false));
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
}

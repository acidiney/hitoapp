#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use hito_core::{Browse, Error, ImportProviderPlaylist, InstallPlugin, OfferPluginService};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn imported_verified_album_with_unknown_credit_is_browsable_without_audio() {
    let server = fixture::ProviderServer::start().await;
    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.release-credit-fixture",
                "name": "Release credit fixture",
                "protocol_version": 1,
                "capabilities": ["playlists"]
            }),
        )
        .await;
    server
        .reply(
            "v1/playlists/detail",
            json!({
                "id": "playlist/verified-album",
                "name": "Provider selection",
                "snapshot": "verified-album-one",
                "total_count": 3,
                "attribution": {"label": "Release credit fixture"}
            }),
        )
        .await;
    let recording = |id: &str, title: &str, track: u32| {
        json!({
            "kind": "recording",
            "recording": {
                "id": id,
                "title": title,
                "artist": "Track performer",
                "duration_ms": 123_000,
                "release": {
                    "id": "album/verified",
                    "title": "Verified provider album"
                },
                "disc_number": 1,
                "track_number": track,
                "revision": "metadata-one"
            }
        })
    };
    let first = recording("recording/first", "First recording", 1);
    let second = recording("recording/second", "Second recording", 2);
    server
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": "playlist/verified-album",
                "snapshot": "verified-album-one",
                "items": [
                    {"occurrence_id": "second-first", "item": second},
                    {"occurrence_id": "first-next", "item": first},
                    {"occurrence_id": "second-again", "item": second}
                ],
                "next_cursor": null
            }),
        )
        .await;
    let (_dir, hito, actor, _shared, _music) = support::library("release-credit-host").await;
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
    let request = || ImportProviderPlaylist {
        request_id,
        installation_revision: installed.revision,
        playlist_reference: "https://provider.invalid/playlists/verified-album".into(),
    };
    let imported = hito
        .collection
        .import_provider_playlist(&actor, &installed.id, request())
        .await
        .expect("verified album identity and title do not require an album artist");
    let playlist = hito
        .collection
        .playlist(&actor, &imported.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(playlist.entries.len(), 3);
    assert_eq!(
        playlist
            .entries
            .iter()
            .map(|entry| (entry.position, entry.title.as_str(), entry.artist.as_str()))
            .collect::<Vec<_>>(),
        [
            (0, "Second recording", "Track performer"),
            (1, "First recording", "Track performer"),
            (2, "Second recording", "Track performer")
        ]
    );
    assert_eq!(
        playlist.entries[0].recording_id,
        playlist.entries[2].recording_id
    );
    assert_ne!(
        playlist.entries[0].recording_id,
        playlist.entries[1].recording_id
    );
    let first_id = playlist.entries[1].recording_id.as_ref().unwrap();
    let second_id = playlist.entries[0].recording_id.as_ref().unwrap();

    let albums = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(albums.total, 1);
    let album = &albums.items[0];
    assert_eq!(album.title, "Verified provider album");
    assert_eq!(album.track_count, 2);
    assert_eq!(album.duration_ms, 246_000);
    let album_json = serde_json::to_value(album).unwrap();
    assert_eq!(album_json.get("artist"), Some(&serde_json::Value::Null));
    assert_eq!(album_json.get("artist_id"), Some(&serde_json::Value::Null));
    let detail = hito
        .catalog
        .release(&actor, &album.id, Browse::default())
        .await
        .unwrap();
    assert_eq!(detail.tracks.total, 2);
    assert_eq!(
        detail
            .tracks
            .items
            .iter()
            .map(|entry| (
                entry.recording.id.as_str(),
                entry.recording.title.as_str(),
                entry.recording.artist.as_str(),
                entry.disc_number,
                entry.track_number,
            ))
            .collect::<Vec<_>>(),
        [
            (
                first_id.as_str(),
                "First recording",
                "Track performer",
                Some(1),
                Some(1)
            ),
            (
                second_id.as_str(),
                "Second recording",
                "Track performer",
                Some(1),
                Some(2)
            )
        ]
    );
    for entry in &detail.tracks.items {
        assert!(!entry.recording.available);
        assert_eq!(entry.recording.album.as_deref(), Some(album.title.as_str()));
        assert!(
            hito.catalog
                .sources(&actor, &entry.recording.id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            hito.downloads.status(&actor, &entry.recording.id).await,
            Err(Error::NotFound)
        ));
    }
    let metadata = serde_json::to_value(
        hito.catalog
            .release_metadata(&actor, &album.id, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        metadata["artist"].get("value"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(
        metadata["artist"].get("inherited"),
        Some(&serde_json::Value::Null)
    );
    let observations = metadata["observations"]["items"].as_array().unwrap();
    assert_eq!(observations.len(), 2);
    assert!(observations.iter().all(|observation| {
        observation["album"] == "Verified provider album"
            && observation["album_artist"].is_null()
            && observation["artist"] == "Track performer"
    }));
    let artists = hito
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 1);
    assert_eq!(artists.items[0].name, "Track performer");

    let replay = hito
        .collection
        .import_provider_playlist(&actor, &installed.id, request())
        .await
        .unwrap();
    assert_eq!(replay.playlist_id, imported.playlist_id);
    let replayed = hito
        .collection
        .playlist(&actor, &replay.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replayed).unwrap(),
        serde_json::to_value(playlist).unwrap()
    );
    let replayed_albums = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(replayed_albums.total, 1);
    assert_eq!(replayed_albums.items[0].id, album.id);
    assert!(matches!(
        hito.downloads
            .playlist_status(
                &actor,
                &imported.playlist_id.to_string(),
                Default::default()
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(
        !hito
            .downloads
            .run_next_job_until(std::future::pending())
            .await
            .unwrap()
    );
    for route in ["v1/recordings/detail", "v1/sources", "v1/audio"] {
        assert!(server.requests(route).await.is_empty());
    }
}

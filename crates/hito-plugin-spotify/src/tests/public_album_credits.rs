use super::{
    Server,
    public_fixture::{Fixture, PLAYLIST_ID, owned_entry},
};
use crate::{Config, router};
use axum::http::StatusCode;
use hito_plugin_sdk::{PlaylistItemsRequest, PlaylistPage, ProviderPlaylist, ProviderPlaylistItem};
use serde_json::{Value, json};
use std::time::Duration;

#[tokio::test]
async fn public_items_preserve_recordings_with_absent_or_null_album() {
    let mut absent = owned_entry(0);
    absent["itemV2"]["data"]
        .as_object_mut()
        .unwrap()
        .remove("albumOfTrack");
    let mut null = owned_entry(1);
    null["itemV2"]["data"]["albumOfTrack"] = Value::Null;
    let fixture = Fixture::with_entries(vec![absent, null]).await;
    let plugin = start_plugin(&fixture).await;
    let client = client();
    let response = request_detail(&client, &plugin).await;
    assert_eq!(response.status(), StatusCode::OK);
    let playlist = response.json::<ProviderPlaylist>().await.unwrap();
    playlist.validate().unwrap();
    assert_eq!(playlist.id, PLAYLIST_ID);
    assert_eq!(playlist.total_count, 2);
    let request = PlaylistItemsRequest {
        playlist_id: playlist.id.clone(),
        snapshot: playlist.snapshot.clone(),
        cursor: None,
        limit: 100,
    };
    let response = client
        .post(plugin.url.join("v1/playlists/items").unwrap())
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page = response.json::<PlaylistPage>().await.unwrap();
    page.validate(request.limit).unwrap();
    assert_eq!(page.playlist_id, playlist.id);
    assert_eq!(page.snapshot, playlist.snapshot);
    assert_eq!(page.items.len(), 2);
    assert!(page.next_cursor.is_none());
    for (entry, number) in page.items.iter().zip(1..=2) {
        assert!(entry.occurrence_id.is_none());
        let ProviderPlaylistItem::Recording { recording } = &entry.item else {
            panic!("Expected the owned recording without an album");
        };
        recording.validate().unwrap();
        assert_eq!(recording.id, format!("{number:022}"));
        assert_eq!(recording.title, format!("Owned recording {number}"));
        assert_eq!(recording.artist, "Owned fixture artist");
        assert_eq!(recording.duration_ms, Some(180_000));
        assert_eq!(recording.disc_number, Some(1));
        assert_eq!(recording.track_number, Some(number));
        assert!(recording.release.is_none());
        assert!(recording.revision.is_none());
    }
    assert_eq!(*fixture.calls.lock().unwrap(), ["embed", "query:0"]);
}

#[tokio::test]
async fn public_detail_rejects_malformed_album_identity_or_title_without_extra_requests() {
    for (label, field, value) in [
        ("missing album URI", "uri", None),
        ("null album URI", "uri", Some(Value::Null)),
        ("non-text album URI", "uri", Some(json!(17))),
        (
            "wrong album namespace",
            "uri",
            Some(json!("spotify:track:0000000000000000000001")),
        ),
        (
            "short album identity",
            "uri",
            Some(json!("spotify:album:1")),
        ),
        (
            "nonalphanumeric album identity",
            "uri",
            Some(json!("spotify:album:000000000000000000000_")),
        ),
        ("missing album title", "name", None),
        ("null album title", "name", Some(Value::Null)),
        ("non-text album title", "name", Some(json!(17))),
        ("blank album title", "name", Some(json!("   "))),
        (
            "control character in album title",
            "name",
            Some(json!("Owned\ninvalid album")),
        ),
        (
            "oversized album title",
            "name",
            Some(json!("x".repeat(2049))),
        ),
    ] {
        let mut entry = owned_entry(0);
        let album = entry["itemV2"]["data"]["albumOfTrack"]
            .as_object_mut()
            .unwrap();
        if let Some(value) = value {
            album.insert(field.to_owned(), value);
        } else {
            album.remove(field);
        }
        let fixture = Fixture::with_entries(vec![entry]).await;
        let plugin = start_plugin(&fixture).await;
        let response = request_detail(&client(), &plugin).await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{label}");
        let body = response.json::<Value>().await.unwrap();
        assert!(
            body == json!({"error": "Spotify returned an invalid playlist response"}),
            "Expected a safe invalid-response error for {label}"
        );
        assert_eq!(
            *fixture.calls.lock().unwrap(),
            ["embed", "query:0"],
            "{label}"
        );
    }
}

async fn start_plugin(fixture: &Fixture) -> Server {
    Server::start(
        router(Config {
            public_base: fixture.server.url.clone(),
            query_endpoint: fixture.server.url.join("pathfinder/v2/query").unwrap(),
        })
        .unwrap(),
    )
    .await
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
}

async fn request_detail(client: &reqwest::Client, plugin: &Server) -> reqwest::Response {
    client
        .post(plugin.url.join("v1/playlists/detail").unwrap())
        .json(&json!({"kind": "canonical", "playlist_id": PLAYLIST_ID}))
        .send()
        .await
        .unwrap()
}

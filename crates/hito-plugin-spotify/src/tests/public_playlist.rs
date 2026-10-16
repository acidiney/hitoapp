use super::{
    Server,
    public_fixture::{Fixture, PLAYLIST_ID},
};
use crate::{Config, router};
use axum::http::StatusCode;
use hito_plugin_sdk::{PlaylistItemsRequest, PlaylistPage, ProviderPlaylist, ProviderPlaylistItem};
use serde_json::json;
use std::{sync::atomic::Ordering, time::Duration};

#[tokio::test]
async fn shared_public_playlist_detail_needs_no_account_and_reads_every_page() {
    let fixture = Fixture::start().await;
    let calls = fixture.calls.clone();
    let spotify = &fixture.server;
    let plugin = Server::start(
        router(Config {
            public_base: spotify.url.clone(),
            query_endpoint: spotify.url.join("pathfinder/v2/query").unwrap(),
            ..Config::default()
        })
        .unwrap(),
    )
    .await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let response = client
        .post(plugin.url.join("v1/playlists/detail").unwrap())
        .json(&json!({
            "kind": "reference",
            "reference": format!("https://open.spotify.com/playlist/{PLAYLIST_ID}?si=ownedtracking")
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let playlist = response.json::<ProviderPlaylist>().await.unwrap();
    playlist.validate().unwrap();
    assert_eq!(playlist.id, PLAYLIST_ID);
    assert_eq!(playlist.name, "Owned public playlist fixture");
    assert_eq!(playlist.total_count, 101);
    assert!(!playlist.snapshot.is_empty());
    assert_eq!(playlist.attribution.label, "Spotify");
    assert_eq!(*calls.lock().unwrap(), ["embed", "query:0", "query:100"]);
}

#[tokio::test]
async fn public_items_pin_order_and_duplicates_while_canonical_detail_refreshes_the_tail() {
    let fixture = Fixture::start().await;
    let plugin = Server::start(
        router(Config {
            public_base: fixture.server.url.clone(),
            query_endpoint: fixture.server.url.join("pathfinder/v2/query").unwrap(),
            ..Config::default()
        })
        .unwrap(),
    )
    .await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let response = client
        .post(plugin.url.join("v1/playlists/detail").unwrap())
        .json(&json!({
            "kind": "reference",
            "reference": format!("https://open.spotify.com/playlist/{PLAYLIST_ID}?si=ownedtracking")
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let original = response.json::<ProviderPlaylist>().await.unwrap();
    original.validate().unwrap();
    let first = read_page(&client, &plugin, &original, None).await;
    assert_eq!(first.items.len(), 100);
    let cursor = first.next_cursor.clone();
    assert!(cursor.is_some());
    let last = read_page(&client, &plugin, &original, cursor.clone()).await;
    assert_eq!(last.items.len(), 1);
    assert!(last.next_cursor.is_none());
    let original_tail = last.items[0].clone();
    let mut entries = first.items;
    entries.extend(last.items);
    assert_eq!(entries.len(), 101);
    for (position, entry) in entries.iter().enumerate() {
        assert!(entry.occurrence_id.is_none());
        let ProviderPlaylistItem::Recording { recording } = &entry.item else {
            panic!("Expected the owned recording fixture");
        };
        recording.validate().unwrap();
        let recording_number = if position == 100 { 1 } else { position + 1 };
        assert_eq!(recording.id, format!("{recording_number:022}"));
        assert_eq!(
            recording.title,
            format!("Owned recording {recording_number}")
        );
        assert_eq!(recording.artist, "Owned fixture artist");
        assert_eq!(recording.duration_ms, Some(180_000));
        assert!(recording.release.is_none());
        assert!(recording.revision.is_none());
    }
    assert!(entries[0] == entries[100]);
    assert_eq!(
        *fixture.calls.lock().unwrap(),
        ["embed", "query:0", "query:100"]
    );

    fixture.tail_changed.store(true, Ordering::SeqCst);
    let response = client
        .post(plugin.url.join("v1/playlists/detail").unwrap())
        .json(&json!({"kind": "canonical", "playlist_id": PLAYLIST_ID}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let current = response.json::<ProviderPlaylist>().await.unwrap();
    current.validate().unwrap();
    assert_eq!(current.id, original.id);
    assert_eq!(current.name, original.name);
    assert_eq!(current.total_count, original.total_count);
    assert_ne!(current.snapshot, original.snapshot);
    let refreshed_first = read_page(&client, &plugin, &current, None).await;
    assert!(refreshed_first.items == entries[..100]);
    let refreshed_last = read_page(&client, &plugin, &current, refreshed_first.next_cursor).await;
    let ProviderPlaylistItem::Recording { recording } = &refreshed_last.items[0].item else {
        panic!("Expected the revised recording fixture");
    };
    assert_eq!(recording.id, format!("{:022}", 999));
    assert_eq!(recording.title, "Owned revised tail recording");
    let pinned_last = read_page(&client, &plugin, &original, cursor).await;
    assert!(pinned_last.items[0] == original_tail);
    assert_eq!(
        *fixture.calls.lock().unwrap(),
        [
            "embed",
            "query:0",
            "query:100",
            "embed",
            "query:0",
            "query:100"
        ]
    );
}

async fn read_page(
    client: &reqwest::Client,
    plugin: &Server,
    playlist: &ProviderPlaylist,
    cursor: Option<String>,
) -> PlaylistPage {
    let request = PlaylistItemsRequest {
        playlist_id: playlist.id.clone(),
        snapshot: playlist.snapshot.clone(),
        cursor,
        limit: 100,
    };
    request.validate().unwrap();
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
    page
}

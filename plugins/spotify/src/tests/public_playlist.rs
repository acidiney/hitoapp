use super::{
    Server,
    public_fixture::{Fixture, PLAYLIST_ID, owned_entry},
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
async fn caller_credentials_do_not_change_the_public_workflow_or_reach_spotify() {
    let fixture = Fixture::start().await;
    let plugin = Server::start(
        router(Config {
            public_base: fixture.server.url.clone(),
            query_endpoint: fixture.server.url.join("pathfinder/v2/query").unwrap(),
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
        .bearer_auth("owned-caller-bearer-token")
        .header("cookie", "owned-caller-cookie=owned-caller-value")
        .json(&json!({
            "kind": "reference",
            "reference": format!("https://open.spotify.com/playlist/{PLAYLIST_ID}")
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
    assert_eq!(
        *fixture.calls.lock().unwrap(),
        ["embed", "query:0", "query:100"]
    );
}

#[tokio::test]
async fn public_items_pin_order_and_duplicates_while_canonical_detail_refreshes_the_tail() {
    let fixture = Fixture::start().await;
    let plugin = Server::start(
        router(Config {
            public_base: fixture.server.url.clone(),
            query_endpoint: fixture.server.url.join("pathfinder/v2/query").unwrap(),
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
        let release = recording
            .release
            .as_ref()
            .expect("Expected the owned verified album");
        assert_eq!(release.id, "0000000000000000000001");
        assert_eq!(release.title, "Owned fixture album");
        assert!(release.artist.is_none());
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

#[tokio::test]
async fn public_items_preserve_unknown_positions_and_reject_malformed_tracks() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    for malformed in [false, true] {
        let mut track = owned_entry(0);
        if malformed {
            track["itemV2"]["data"]
                .as_object_mut()
                .unwrap()
                .remove("uri");
        }
        let fixture = Fixture::with_entries(vec![
            track,
            json!({"itemV2": {"__typename": "PlaylistTrack", "data": null}}),
            json!({"itemV2": {"__typename": "PlaylistFutureItem", "data": {
                "__typename": "FutureContent", "name": "Owned unsupported item"
            }}}),
            owned_entry(1),
        ])
        .await;
        let plugin = Server::start(
            router(Config {
                public_base: fixture.server.url.clone(),
                query_endpoint: fixture.server.url.join("pathfinder/v2/query").unwrap(),
            })
            .unwrap(),
        )
        .await;
        let response = client
            .post(plugin.url.join("v1/playlists/detail").unwrap())
            .json(&json!({"kind": "canonical", "playlist_id": PLAYLIST_ID}))
            .send()
            .await
            .unwrap();
        if malformed {
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            assert_eq!(
                response.json::<serde_json::Value>().await.unwrap(),
                json!({
                    "error": "Spotify returned an invalid playlist response"
                })
            );
        } else {
            assert_eq!(response.status(), StatusCode::OK);
            let playlist = response.json::<ProviderPlaylist>().await.unwrap();
            playlist.validate().unwrap();
            assert_eq!(playlist.total_count, 4);
            let page = read_page(&client, &plugin, &playlist, None).await;
            assert_eq!(page.items.len(), 4);
            assert!(page.next_cursor.is_none());
            assert!(page.items.iter().all(|entry| entry.occurrence_id.is_none()));
            assert!(matches!(
                page.items[1].item,
                ProviderPlaylistItem::Unknown {}
            ));
            assert!(matches!(
                page.items[2].item,
                ProviderPlaylistItem::Unknown {}
            ));
            for (position, number) in [(0, 1), (3, 2)] {
                let ProviderPlaylistItem::Recording { recording } = &page.items[position].item
                else {
                    panic!("Expected an owned recording at its original position");
                };
                assert_eq!(recording.id, format!("{number:022}"));
                recording.validate().unwrap();
            }
        }
        assert_eq!(*fixture.calls.lock().unwrap(), ["embed", "query:0"]);
    }
}

#[tokio::test]
async fn public_items_preserve_verified_album_identity_and_recording_occurrences() {
    let fixture = Fixture::with_entries(vec![owned_entry(0), owned_entry(1), owned_entry(0)]).await;
    let plugin = Server::start(
        router(Config {
            public_base: fixture.server.url.clone(),
            query_endpoint: fixture.server.url.join("pathfinder/v2/query").unwrap(),
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
        .json(&json!({"kind": "canonical", "playlist_id": PLAYLIST_ID}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let playlist = response.json::<ProviderPlaylist>().await.unwrap();
    playlist.validate().unwrap();
    assert_eq!(playlist.id, PLAYLIST_ID);
    assert_eq!(playlist.total_count, 3);
    let page = read_page(&client, &plugin, &playlist, None).await;
    assert_eq!(page.items.len(), 3);
    assert!(page.next_cursor.is_none());
    assert!(page.items[0] == page.items[2]);
    for (entry, number) in page.items.iter().zip([1, 2, 1]) {
        assert!(entry.occurrence_id.is_none());
        let ProviderPlaylistItem::Recording { recording } = &entry.item else {
            panic!("Expected an owned recording at its original position");
        };
        recording.validate().unwrap();
        assert_eq!(recording.id, format!("{number:022}"));
        assert_eq!(recording.title, format!("Owned recording {number}"));
        assert_eq!(recording.artist, "Owned fixture artist");
        assert_eq!(recording.duration_ms, Some(180_000));
        assert_eq!(recording.disc_number, Some(1));
        assert_eq!(recording.track_number, Some(number));
        assert!(recording.revision.is_none());
    }
    assert_eq!(*fixture.calls.lock().unwrap(), ["embed", "query:0"]);
    for entry in &page.items {
        let ProviderPlaylistItem::Recording { recording } = &entry.item else {
            panic!("Expected an owned recording at its original position");
        };
        let Some(release) = &recording.release else {
            panic!("The provider's verified album identity and title must be preserved");
        };
        assert_eq!(release.id, "0000000000000000000001");
        assert_eq!(release.title, "Owned fixture album");
        assert!(release.artist.is_none());
    }
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

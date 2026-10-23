use super::{
    Server,
    public_fixture::{PLAYLIST_ID, bootstrap_html, owned_entry},
};
use crate::{Config, router};
use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use hito_plugin_sdk::{PlaylistPage, ProviderPlaylist, ProviderPlaylistEntry};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[tokio::test]
async fn failed_later_pages_preserve_the_last_complete_snapshot() {
    for (failure, expected) in [
        ("truncated", StatusCode::BAD_GATEWAY),
        ("inconsistent", StatusCode::BAD_GATEWAY),
        ("failed", StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let fail = Arc::new(AtomicBool::new(false));
        let refreshing = fail.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let embed_calls = calls.clone();
        let query_calls = calls.clone();
        let upstream = Server::start(
            Router::new()
                .route(
                    "/embed/playlist/0000000000000000000001",
                    get(move |headers: HeaderMap| {
                        let calls = embed_calls.clone();
                        async move {
                            assert!(headers.get("authorization").is_none());
                            assert!(headers.get("cookie").is_none());
                            calls.lock().unwrap().push("embed".to_owned());
                            bootstrap_html("owned-pagination-token")
                        }
                    }),
                )
                .route(
                    "/pathfinder/v2/query",
                    post(move |headers: HeaderMap, Json(body): Json<Value>| {
                        let calls = query_calls.clone();
                        let refreshing = refreshing.clone();
                        async move {
                            assert!(
                                headers
                                    .get("authorization")
                                    .is_some_and(|value| value == "Bearer owned-pagination-token")
                            );
                            assert!(headers.get("cookie").is_none());
                            let offset = body["variables"]["offset"].as_u64().unwrap();
                            assert!(offset == 0 || offset == 100);
                            calls.lock().unwrap().push(format!("query:{offset}"));
                            let failed_tail = refreshing.load(Ordering::SeqCst) && offset == 100;
                            if failed_tail && failure == "failed" {
                                return (
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    "private-owned-later-page-body",
                                )
                                    .into_response();
                            }
                            let items: Vec<_> = if failed_tail && failure == "truncated" {
                                Vec::new()
                            } else {
                                (offset..(offset + 100).min(101)).map(owned_entry).collect()
                            };
                            let total = if failed_tail && failure == "inconsistent" {
                                102
                            } else {
                                101
                            };
                            Json(json!({"data": {"playlistV2": {
                        "__typename": "Playlist",
                        "uri": format!("spotify:playlist:{PLAYLIST_ID}"),
                        "name": "Owned pagination recovery fixture",
                        "content": {
                            "items": items,
                            "totalCount": total,
                            "pagingInfo": {"nextOffset": if offset == 0 { Some(100) } else { None }}
                        }
                    }}})).into_response()
                        }
                    }),
                ),
        )
        .await;
        let plugin = Server::start(
            router(Config {
                public_base: upstream.url.clone(),
                query_endpoint: upstream.url.join("pathfinder/v2/query").unwrap(),
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
        assert_eq!(playlist.total_count, 101);
        let original = pinned_entries(&client, &plugin, &playlist).await;
        assert_eq!(original.len(), 101);
        assert!(original[0] == original[100]);

        fail.store(true, Ordering::SeqCst);
        let response = client
            .post(plugin.url.join("v1/playlists/detail").unwrap())
            .json(&json!({"kind": "canonical", "playlist_id": PLAYLIST_ID}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "later page case: {failure}");
        let error = if expected == StatusCode::SERVICE_UNAVAILABLE {
            "Spotify could not be reached"
        } else {
            "Spotify returned an invalid playlist response"
        };
        assert_eq!(
            response.json::<Value>().await.unwrap(),
            json!({"error": error})
        );
        let preserved = pinned_entries(&client, &plugin, &playlist).await;
        assert!(preserved == original);

        let unknown = format!(
            "{}{}",
            if playlist.snapshot.starts_with('0') {
                '1'
            } else {
                '0'
            },
            &playlist.snapshot[1..]
        );
        for (snapshot, cursor, expected) in [
            (
                playlist.snapshot.clone(),
                Some("01"),
                StatusCode::BAD_REQUEST,
            ),
            (unknown, None, StatusCode::GONE),
        ] {
            let response = client
                .post(plugin.url.join("v1/playlists/items").unwrap())
                .json(&json!({"playlist_id": PLAYLIST_ID, "snapshot": snapshot, "cursor": cursor, "limit": 100}))
                .send().await.unwrap();
            assert_eq!(response.status(), expected);
            let error = if expected == StatusCode::GONE {
                "This playlist snapshot is no longer available. Import the playlist again"
            } else {
                "The playlist request is invalid"
            };
            assert_eq!(
                response.json::<Value>().await.unwrap(),
                json!({"error": error})
            );
        }
        assert_eq!(
            *calls.lock().unwrap(),
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
}

async fn pinned_entries(
    client: &reqwest::Client,
    plugin: &Server,
    playlist: &ProviderPlaylist,
) -> Vec<ProviderPlaylistEntry> {
    let mut entries = Vec::new();
    let mut cursor = None;
    for expected_count in [100, 1] {
        let response = client
            .post(plugin.url.join("v1/playlists/items").unwrap())
            .json(&json!({"playlist_id": playlist.id, "snapshot": playlist.snapshot, "cursor": cursor, "limit": 100}))
            .send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let page = response.json::<PlaylistPage>().await.unwrap();
        page.validate(100).unwrap();
        assert_eq!(page.playlist_id, playlist.id);
        assert_eq!(page.snapshot, playlist.snapshot);
        assert_eq!(page.items.len(), expected_count);
        assert!(page.items.iter().all(|entry| entry.occurrence_id.is_none()));
        cursor = page.next_cursor;
        assert_eq!(cursor.is_some(), expected_count == 100);
        entries.extend(page.items);
    }
    entries
}

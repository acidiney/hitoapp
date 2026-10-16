use super::{
    Server,
    public_fixture::{PLAYLIST_ID, bootstrap_html},
};
use crate::{Config, router};
use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use hito_plugin_sdk::{PlaylistPage, ProviderPlaylist};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[tokio::test]
async fn inaccessible_playlist_contents_are_distinct_from_an_empty_playlist() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    for (case, content, upstream_status, expected) in [
        ("missing", None, StatusCode::OK, StatusCode::BAD_GATEWAY),
        (
            "null",
            Some(json!(null)),
            StatusCode::OK,
            StatusCode::BAD_GATEWAY,
        ),
        (
            "forbidden",
            None,
            StatusCode::FORBIDDEN,
            StatusCode::FORBIDDEN,
        ),
        (
            "empty",
            Some(json!({
                "items": [], "totalCount": 0, "pagingInfo": {"nextOffset": null}
            })),
            StatusCode::OK,
            StatusCode::OK,
        ),
    ] {
        let mut playlist = json!({
            "__typename": "Playlist",
            "uri": format!("spotify:playlist:{PLAYLIST_ID}"),
            "name": "Owned public access fixture"
        });
        if let Some(content) = content {
            playlist
                .as_object_mut()
                .unwrap()
                .insert("content".into(), content);
        }
        let metadata = json!({"data": {"playlistV2": playlist}});
        let calls = Arc::new(AtomicUsize::new(0));
        let embed_calls = calls.clone();
        let query_calls = calls.clone();
        let spotify =
            Server::start(
                Router::new()
                    .route(
                        "/embed/playlist/0000000000000000000001",
                        get(move |headers: HeaderMap| {
                            let calls = embed_calls.clone();
                            async move {
                                assert!(headers.get("authorization").is_none());
                                assert!(headers.get("cookie").is_none());
                                calls.fetch_add(1, Ordering::SeqCst);
                                bootstrap_html("owned-public-access-token")
                            }
                        }),
                    )
                    .route(
                        "/pathfinder/v2/query",
                        post(move |headers: HeaderMap| {
                            let calls = query_calls.clone();
                            let metadata = metadata.clone();
                            async move {
                                assert!(headers.get("authorization").is_some_and(
                                    |value| value == "Bearer owned-public-access-token"
                                ));
                                assert!(headers.get("cookie").is_none());
                                calls.fetch_add(1, Ordering::SeqCst);
                                if upstream_status != StatusCode::OK {
                                    return (upstream_status, "owned-inaccessible-body")
                                        .into_response();
                                }
                                Json(metadata).into_response()
                            }
                        }),
                    ),
            )
            .await;
        let plugin = Server::start(
            router(Config {
                public_base: spotify.url.clone(),
                query_endpoint: spotify.url.join("pathfinder/v2/query").unwrap(),
            })
            .unwrap(),
        )
        .await;
        let response = client
            .post(plugin.url.join("v1/playlists/detail").unwrap())
            .json(&json!({"kind": "reference", "reference": format!("https://open.spotify.com/playlist/{PLAYLIST_ID}")}))
            .send().await.unwrap();

        assert_eq!(response.status(), expected, "content case: {case}");
        if expected == StatusCode::OK {
            let playlist = response.json::<ProviderPlaylist>().await.unwrap();
            playlist.validate().unwrap();
            assert_eq!(playlist.id, PLAYLIST_ID);
            assert!(!playlist.snapshot.is_empty());
            assert_eq!(playlist.total_count, 0);
            let response = client
                .post(plugin.url.join("v1/playlists/items").unwrap())
                .json(&json!({"playlist_id": playlist.id, "snapshot": playlist.snapshot, "cursor": null, "limit": 100}))
                .send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let page = response.json::<PlaylistPage>().await.unwrap();
            page.validate(100).unwrap();
            assert!(page.items.is_empty());
            assert!(page.next_cursor.is_none());
        } else {
            let error = if expected == StatusCode::FORBIDDEN {
                "Spotify does not allow public access to this playlist"
            } else {
                "Spotify returned an invalid playlist response"
            };
            assert_eq!(
                response.json::<serde_json::Value>().await.unwrap(),
                json!({"error": error})
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2, "content case: {case}");
    }
}

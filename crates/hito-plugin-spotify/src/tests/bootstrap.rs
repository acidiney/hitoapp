use super::{
    Server,
    public_fixture::{PLAYLIST_ID, bootstrap_html},
};
use crate::{Config, router};
use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, post},
};
use hito_plugin_sdk::ProviderPlaylist;
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[tokio::test]
async fn bounded_bootstrap_with_many_attributes_allows_repeated_public_reads() {
    let attributes = format!("{}{}>", "a ".repeat(250_000), " ".repeat(1_000_000));
    let html = bootstrap_html("owned-large-bootstrap-token").0.replacen(
        "type=\"application/json\">",
        &format!("type=\"application/json\" {attributes}"),
        1,
    );
    assert!(html.len() < 2 * 1024 * 1024);
    let calls = Arc::new(AtomicUsize::new(0));
    let embed_calls = calls.clone();
    let query_calls = calls.clone();
    let upstream = Server::start(Router::new()
        .route("/embed/playlist/0000000000000000000001", get(move |headers: HeaderMap| {
            let html = html.clone();
            let calls = embed_calls.clone();
            async move {
                assert!(headers.get("authorization").is_none());
                assert!(headers.get("cookie").is_none());
                calls.fetch_add(1, Ordering::SeqCst);
                Html(html)
            }
        }))
        .route("/pathfinder/v2/query", post(move |headers: HeaderMap| {
            let calls = query_calls.clone();
            async move {
                assert!(headers.get("authorization").is_some_and(|value| value == "Bearer owned-large-bootstrap-token"));
                assert!(headers.get("cookie").is_none());
                calls.fetch_add(1, Ordering::SeqCst);
                Json(json!({"data": {"playlistV2": {
                    "__typename": "Playlist",
                    "uri": format!("spotify:playlist:{PLAYLIST_ID}"),
                    "name": "Owned large bootstrap fixture",
                    "content": {"items": [], "totalCount": 0, "pagingInfo": {"nextOffset": null}}
                }}}))
            }
        })),
    ).await;
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
    let mut previous = None;
    for completed in 1..=2 {
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
        assert_eq!(playlist.total_count, 0);
        assert!(!playlist.snapshot.is_empty());
        if let Some(previous) = previous.as_ref() {
            assert_eq!(&playlist.snapshot, previous);
        }
        previous = Some(playlist.snapshot);
        assert_eq!(calls.load(Ordering::SeqCst), completed * 2);
    }
}

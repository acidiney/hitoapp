use super::Server;
use crate::{Config, router};
use axum::{Router, http::StatusCode};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[tokio::test]
async fn noncanonical_playlist_references_are_rejected_without_an_upstream_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let spotify = Server::start(Router::new().fallback(move || {
        let observed = observed.clone();
        async move {
            observed.fetch_add(1, Ordering::SeqCst);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }))
    .await;
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

    for (case, reference) in [
        (
            "explicit default port",
            "https://open.spotify.com:443/playlist/0000000000000000000001",
        ),
        (
            "literal dot segment",
            "https://open.spotify.com/artist/../playlist/0000000000000000000001",
        ),
        (
            "encoded dot segment",
            "https://open.spotify.com/artist/%2e%2e/playlist/0000000000000000000001",
        ),
        (
            "scheme and host case",
            "HTTPS://OPEN.SPOTIFY.COM/playlist/0000000000000000000001",
        ),
        (
            "backslashes",
            r"https:\\open.spotify.com\playlist\0000000000000000000001",
        ),
        (
            "unrelated host",
            "https://spotify.example/playlist/0000000000000000000001",
        ),
        (
            "fragment",
            "https://open.spotify.com/playlist/0000000000000000000001#fixture",
        ),
        (
            "credentials",
            "https://fixture@open.spotify.com/playlist/0000000000000000000001",
        ),
    ] {
        let response = client
            .post(plugin.url.join("v1/playlists/detail").unwrap())
            .json(&json!({"kind": "reference", "reference": reference}))
            .send()
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "reference case: {case}; upstream calls: {}",
            calls.load(Ordering::SeqCst)
        );
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap(),
            json!({
                "error": "The playlist request is invalid"
            })
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "reference case: {case}");
    }
}

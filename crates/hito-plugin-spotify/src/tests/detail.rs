use super::Server;
use crate::{Config, router};
use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    routing::get,
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
async fn canonical_playlist_link_returns_current_spotify_metadata() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let spotify = Server::start(Router::new().route(
        "/v1/playlists/0000000000000000000001",
        get(move |headers: HeaderMap| {
            let observed = observed.clone();
            async move {
                if !headers.get("authorization").is_some_and(|value| value == "Bearer owned-fixture-access-token") {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Json(json!({
                    "id": "0000000000000000000001",
                    "name": "Owned playlist fixture",
                    "type": "playlist",
                    "snapshot_id": "owned-snapshot-v1",
                    "external_urls": {"spotify": "https://open.spotify.com/playlist/0000000000000000000001"},
                    "owner": {"id": "fixture-owner", "type": "user"},
                    "items": {"total": 3}
                })))
            }
        }),
    )).await;
    let plugin = Server::start(
        router(Config {
            api_base: spotify.url.join("v1/").unwrap(),
            ..Config::default()
        })
        .unwrap(),
    )
    .await;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let response = client.post(plugin.url.join("v1/playlists/detail").unwrap())
        .bearer_auth("owned-fixture-access-token")
        .json(&json!({"kind": "reference", "reference": "https://open.spotify.com/playlist/0000000000000000000001"}))
        .send().await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let playlist = response.json::<ProviderPlaylist>().await.unwrap();
    playlist.validate().unwrap();
    assert_eq!(playlist.id, "0000000000000000000001");
    assert_eq!(playlist.name, "Owned playlist fixture");
    assert_eq!(playlist.snapshot, "owned-snapshot-v1");
    assert_eq!(playlist.total_count, 3);
    assert_eq!(playlist.attribution.label, "Spotify");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

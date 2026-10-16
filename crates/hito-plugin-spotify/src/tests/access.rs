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
async fn inaccessible_playlist_contents_are_distinct_from_an_empty_playlist() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    for (case, items, expected) in [
        ("missing", None, StatusCode::FORBIDDEN),
        ("null", Some(json!(null)), StatusCode::FORBIDDEN),
        ("empty", Some(json!({"total": 0})), StatusCode::OK),
    ] {
        let mut metadata = json!({
            "id": "0000000000000000000001",
            "name": "Private owned fixture",
            "snapshot_id": "owned-snapshot-v1"
        });
        if let Some(items) = items {
            metadata
                .as_object_mut()
                .unwrap()
                .insert("items".into(), items);
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let spotify = Server::start(Router::new().route(
            "/v1/playlists/0000000000000000000001",
            get(move |headers: HeaderMap| {
                let metadata = metadata.clone();
                let observed = observed.clone();
                async move {
                    if !headers
                        .get("authorization")
                        .is_some_and(|value| value == "Bearer owned-fixture-access-token")
                    {
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                    observed.fetch_add(1, Ordering::SeqCst);
                    Ok(Json(metadata))
                }
            }),
        ))
        .await;
        let plugin = Server::start(
            router(Config {
                api_base: spotify.url.join("v1/").unwrap(),
                ..Config::default()
            })
            .unwrap(),
        )
        .await;
        let response = client.post(plugin.url.join("v1/playlists/detail").unwrap())
            .bearer_auth("owned-fixture-access-token")
            .json(&json!({"kind": "reference", "reference": "https://open.spotify.com/playlist/0000000000000000000001"}))
            .send().await.unwrap();

        assert_eq!(response.status(), expected, "items case: {case}");
        if expected == StatusCode::OK {
            let playlist = response.json::<ProviderPlaylist>().await.unwrap();
            playlist.validate().unwrap();
            assert_eq!(playlist.id, "0000000000000000000001");
            assert_eq!(playlist.snapshot, "owned-snapshot-v1");
            assert_eq!(playlist.total_count, 0);
        } else {
            assert_eq!(
                response.json::<serde_json::Value>().await.unwrap(),
                json!({
                    "error": "Spotify does not allow this account to read the playlist contents"
                })
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "items case: {case}");
    }
}

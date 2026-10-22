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
use hito_plugin_sdk::ProviderPlaylist;
use hito_testkit::logs::LogCapture;
use serde_json::json;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

#[tokio::test]
async fn workflow_completion_logs_are_useful_at_info_and_private_at_trace() {
    let _ambient_no_subscriber_dispatch =
        tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    for level in [tracing::Level::INFO, tracing::Level::TRACE] {
        let logs = LogCapture::unbounded();
        let captured = logs.clone();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::filter::filter_fn(|metadata| {
                metadata.target() == "hito_plugin_spotify"
                    || metadata.target().starts_with("hito_plugin_spotify::")
            }))
            .with(tracing_subscriber::filter::LevelFilter::from_level(level))
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_writer(move || captured.clone()),
            );
        let dispatch = tracing::Dispatch::new(subscriber);
        let fail = Arc::new(AtomicBool::new(false));
        let failure = fail.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let embed_calls = calls.clone();
        let query_calls = calls.clone();
        let spotify = Server::start(Router::new()
            .route("/embed/playlist/0000000000000000000001", get(move |headers: HeaderMap| {
                let calls = embed_calls.clone();
                async move {
                    assert!(headers.get("authorization").is_none());
                    assert!(headers.get("cookie").is_none());
                    assert!(headers.get("x-owned-private").is_none());
                    calls.fetch_add(1, Ordering::SeqCst);
                    bootstrap_html("private-anonymous-session-sentinel")
                }
            }))
            .route("/pathfinder/v2/query", post(move |headers: HeaderMap| {
                let failure = failure.clone();
                let observed = query_calls.clone();
                async move {
                    assert!(headers.get("authorization").is_some_and(|value| value == "Bearer private-anonymous-session-sentinel"));
                    assert!(headers.get("cookie").is_none());
                    assert!(headers.get("x-owned-private").is_none());
                    observed.fetch_add(1, Ordering::SeqCst);
                    if failure.load(Ordering::SeqCst) {
                        return (
                            StatusCode::SERVICE_UNAVAILABLE,
                            "private-upstream-body-sentinel",
                        )
                            .into_response();
                    }
                    Json(json!({
                        "data": {"playlistV2": {
                            "__typename": "Playlist",
                            "uri": format!("spotify:playlist:{PLAYLIST_ID}"),
                            "name": "private-playlist-title-sentinel",
                            "content": {"items": [], "totalCount": 0, "pagingInfo": {"nextOffset": null}}
                        }}
                    }))
                    .into_response()
                }
            })))
        .await;
        let app = router(Config {
            public_base: spotify.url.clone(),
            query_endpoint: spotify.url.join("pathfinder/v2/query").unwrap(),
        })
        .unwrap()
        .layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let dispatch = dispatch.clone();
                async move { next.run(request).with_subscriber(dispatch).await }
            },
        ));
        let plugin = Server::start(app).await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let reference = "https://open.spotify.com/playlist/0000000000000000000001";
        let mut endpoint = plugin.url.join("v1/playlists/detail").unwrap();
        endpoint.set_query(Some("private-query-sentinel=private-query-value-sentinel"));
        let response = client
            .post(endpoint.clone())
            .bearer_auth("private-playlist-bearer-sentinel")
            .header(
                "cookie",
                "private-cookie-sentinel=private-cookie-value-sentinel",
            )
            .header("x-owned-private", "private-header-sentinel")
            .json(&json!({"kind": "reference", "reference": reference}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response
            .json::<ProviderPlaylist>()
            .await
            .unwrap()
            .validate()
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let response = client
            .post(endpoint.clone())
            .bearer_auth("private-playlist-bearer-sentinel")
            .header("content-type", "application/json")
            .body(r#"{"kind":"reference","reference":"private-malformed-json-sentinel",]"#)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap(),
            json!({
                "error": "The playlist request is invalid"
            })
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        fail.store(true, Ordering::SeqCst);
        let response = client
            .post(endpoint)
            .bearer_auth("private-playlist-bearer-sentinel")
            .json(&json!({"kind": "reference", "reference": reference}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap(),
            json!({
                "error": "Spotify could not be reached"
            })
        );
        assert_eq!(calls.load(Ordering::SeqCst), 4);

        let output = logs.text();
        assert!(
            !output.contains("private-"),
            "Workflow logs exposed private content"
        );
        assert!(
            !output.contains(reference),
            "Workflow logs exposed a playlist reference"
        );
        let entries: Vec<serde_json::Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let observed: Vec<_> = entries
            .iter()
            .map(|entry| {
                let event = match entry["fields"]["event"].as_str() {
                    Some("spotify_request_complete") => "spotify_request_complete",
                    Some("spotify_response_headers") => "spotify_response_headers",
                    Some("spotify_playlist_normalized") => "spotify_playlist_normalized",
                    _ => "<other>",
                };
                let spans: Vec<_> = entry["spans"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|span| {
                        let name = match span["name"].as_str() {
                            Some("spotify_request") => "spotify_request",
                            _ => "<other>",
                        };
                        (name, span["request_id"].as_u64())
                    })
                    .collect();
                (event, entry["fields"]["status"].as_u64(), spans)
            })
            .collect();
        let mut request_ids = HashSet::new();
        for status in [200, 400, 503] {
            let completion = entries
                .iter()
                .find(|entry| {
                    entry["fields"]["event"] == "spotify_request_complete"
                        && entry["fields"]["status"] == status
                })
                .unwrap_or_else(|| panic!("Missing workflow completion at level {level} for status {status}; sanitized observed events: {observed:?}"));
            assert!(completion["fields"]["elapsed_ms"].is_number());
            let span = completion["spans"]
                .as_array()
                .unwrap()
                .iter()
                .find(|span| span["name"] == "spotify_request")
                .expect("Workflow completion needs its request correlation span");
            assert_eq!(span["operation"], "playlist_detail");
            request_ids.insert(
                span["request_id"]
                    .as_u64()
                    .expect("Request correlation needs a numeric ID"),
            );
        }
        assert_eq!(request_ids.len(), 3);
    }
}

use crate::{Config, router};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use hito_plugin_sdk::SearchPage;
use hito_testkit::logs::LogCapture;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{collections::HashSet, fs, os::unix::fs::PermissionsExt};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

#[tokio::test]
async fn request_completions_are_useful_at_info_and_private_at_trace() {
    let _ambient_no_subscriber_dispatch =
        tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    for level in [tracing::Level::INFO, tracing::Level::TRACE] {
        let logs = LogCapture::unbounded();
        let captured = logs.clone();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::filter::filter_fn(|metadata| {
                metadata.target() == "hito_plugin_youtube"
                    || metadata.target().starts_with("hito_plugin_youtube::")
            }))
            .with(tracing_subscriber::filter::LevelFilter::from_level(level))
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_writer(move || captured.clone()),
            );
        let dispatch = tracing::Dispatch::new(subscriber);
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("private-executable-path-sentinel");
        fs::write(&executable, include_str!("../fixtures/logging.sh")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let private_path = executable.to_string_lossy().into_owned();
        let app = router(Config { executable });
        let query = json!({"query":"private-search-query-sentinel","cursor":null,"limit":1});

        let response = request(&app, &dispatch, &query.to_string()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
        page.validate(1).unwrap();
        assert_eq!(page.items[0].title, "private-title-sentinel");
        assert_eq!(
            fs::read_to_string(directory.path().join("calls"))
                .unwrap()
                .lines()
                .count(),
            1
        );

        let response = request(
            &app,
            &dispatch,
            r#"{"query":"private-malformed-json-sentinel",]"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({"error":"The search request is invalid"})
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("calls"))
                .unwrap()
                .lines()
                .count(),
            1
        );

        fs::write(directory.path().join("fail"), []).unwrap();
        let response = request(&app, &dispatch, &query.to_string()).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({"error":"The YouTube service is unavailable"})
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("calls"))
                .unwrap()
                .lines()
                .count(),
            2
        );

        let output = logs.text();
        assert!(
            !output.contains("private-"),
            "Workflow logs exposed private content"
        );
        assert!(
            !output.contains(&private_path),
            "Workflow logs exposed the executable path"
        );
        assert!(
            !output.contains("https://audio.invalid/"),
            "Workflow logs exposed an upstream address"
        );
        let entries: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let mut ids = HashSet::new();
        for status in [200, 400, 503] {
            let completion = entries
                .iter()
                .find(|entry| {
                    entry["fields"]["event"] == "youtube_request_complete"
                        && entry["fields"]["status"] == status
                })
                .unwrap_or_else(|| {
                    panic!("Missing request completion at level {level} for status {status}")
                });
            assert_eq!(completion["level"], "INFO");
            assert!(completion["fields"]["elapsed_ms"].as_u64().is_some());
            let id = correlation(completion);
            ids.insert(id);
            if status != 400 {
                let outcome = if status == 200 {
                    "complete"
                } else {
                    "unavailable"
                };
                let worker = entries.iter().find(|entry| {
                    entry["fields"]["event"] == "youtube_search_complete"
                        && entry["fields"]["outcome"] == outcome
                        && correlation(entry) == id
                }).unwrap_or_else(|| panic!("Missing correlated extraction completion at level {level} for status {status}"));
                assert_eq!(worker["level"], "INFO");
                assert!(worker["fields"]["elapsed_ms"].as_u64().is_some());
            }
        }
        assert_eq!(ids.len(), 3);
    }
}

async fn request(app: &Router, dispatch: &tracing::Dispatch, payload: &str) -> Response {
    app.clone()
        .oneshot(
            Request::post("/v1/search?private-uri-query-sentinel=private-uri-value-sentinel")
                .header("content-type", "application/json")
                .header("authorization", "Bearer private-bearer-sentinel")
                .header(
                    "cookie",
                    "private-cookie-sentinel=private-cookie-value-sentinel",
                )
                .header("x-owned-private", "private-header-sentinel")
                .body(Body::from(payload.to_owned()))
                .unwrap(),
        )
        .with_subscriber(dispatch.clone())
        .await
        .unwrap()
}

fn correlation(entry: &Value) -> u64 {
    let span = entry["spans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|span| span["name"] == "youtube_request")
        .expect("Workflow completion needs its request correlation span");
    assert_eq!(span["operation"], "search");
    span["request_id"]
        .as_u64()
        .expect("Request correlation needs a numeric ID")
}

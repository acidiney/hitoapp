use crate::{Config, router};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_plugin_sdk::{HealthReason, HealthReport, HealthState, MAX_HEALTH_BYTES};
use hito_testkit::logs::LogCapture;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

const NETWORK_STDERR: &str =
    "ERROR: [youtube] private-stderr-sentinel: Temporary failure in name resolution";
const REFUSED_STDERR: &str =
    "ERROR: [youtube] private-stderr-sentinel: Sign in to confirm you're not a bot";

fn service(directory: &Path) -> Router {
    let executable = directory.join("owned-yt-dlp");
    fs::write(&executable, include_str!("../fixtures/switching.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    router(Config { executable })
}

fn fail_with(directory: &Path, stderr: &str) {
    fs::write(directory.join("stderr"), stderr).unwrap();
    fs::write(directory.join("fail"), []).unwrap();
}

async fn search(app: &Router) -> StatusCode {
    app.clone()
        .oneshot(
            Request::post("/v1/search")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"query":"private-query-sentinel","cursor":null,"limit":1}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn health(app: &Router) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(Request::get("/v1/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    (
        status,
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
}

async fn report(app: &Router) -> HealthReport {
    let (status, bytes) = health(app).await;
    assert_eq!(status, StatusCode::OK);
    assert!(bytes.len() <= MAX_HEALTH_BYTES);
    let report: HealthReport = serde_json::from_slice(&bytes).unwrap();
    report.validate().unwrap();
    report
}

#[tokio::test]
async fn a_fresh_service_is_healthy_and_the_probe_never_starts_the_executable() {
    let directory = tempfile::tempdir().unwrap();
    let app = service(directory.path());
    assert_eq!(report(&app).await, HealthReport::ok());
    let response = app
        .clone()
        .oneshot(Request::get("/v1/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(!directory.path().join("calls").exists());
}

#[tokio::test]
async fn three_network_failures_degrade_health_and_one_success_recovers_it() {
    let directory = tempfile::tempdir().unwrap();
    let app = service(directory.path());
    fail_with(directory.path(), NETWORK_STDERR);
    for _ in 0..2 {
        assert_eq!(search(&app).await, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(report(&app).await, HealthReport::ok());
    }
    assert_eq!(search(&app).await, StatusCode::SERVICE_UNAVAILABLE);
    let degraded = report(&app).await;
    assert_eq!(degraded.state, HealthState::Degraded);
    assert_eq!(degraded.reason, Some(HealthReason::Network));
    let since = degraded.since.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(since <= now && now - since < 60);

    fs::remove_file(directory.path().join("fail")).unwrap();
    assert_eq!(search(&app).await, StatusCode::OK);
    assert_eq!(report(&app).await, HealthReport::ok());
}

#[tokio::test]
async fn repeated_refusals_report_refused() {
    let directory = tempfile::tempdir().unwrap();
    let app = service(directory.path());
    fail_with(directory.path(), REFUSED_STDERR);
    for _ in 0..3 {
        search(&app).await;
    }
    assert_eq!(report(&app).await.reason, Some(HealthReason::Refused));
}

#[tokio::test]
async fn unclassified_failures_and_mixed_classes_never_degrade_health() {
    let directory = tempfile::tempdir().unwrap();
    let app = service(directory.path());
    fail_with(directory.path(), "ERROR: something nobody has classified");
    for _ in 0..5 {
        search(&app).await;
    }
    assert_eq!(report(&app).await, HealthReport::ok());
    for stderr in [
        NETWORK_STDERR,
        REFUSED_STDERR,
        NETWORK_STDERR,
        REFUSED_STDERR,
    ] {
        fail_with(directory.path(), stderr);
        search(&app).await;
    }
    assert_eq!(report(&app).await, HealthReport::ok());
}

#[tokio::test]
async fn health_exposes_only_the_fixed_fields_and_logs_no_private_text() {
    let _ambient_no_subscriber_dispatch =
        tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let logs = LogCapture::unbounded();
    let captured = logs.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::registry()
            .with(tracing_subscriber::filter::LevelFilter::TRACE)
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_writer(move || captured.clone()),
            ),
    );
    let directory = tempfile::tempdir().unwrap();
    let app = service(directory.path());
    fail_with(directory.path(), NETWORK_STDERR);
    async {
        for _ in 0..3 {
            search(&app).await;
        }
        let (_, bytes) = health(&app).await;
        let body = std::str::from_utf8(&bytes).unwrap();
        assert!(!body.contains("private-"));
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["reason", "since", "state"]);
    }
    .with_subscriber(dispatch)
    .await;
    let output = logs.text();
    assert!(!output.contains("private-"));
    assert!(!output.contains("name resolution"));
    let completion = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|entry| {
            entry["fields"]["event"] == "youtube_request_complete"
                && entry["span"]["operation"] == "health"
        })
        .expect("health request completion");
    assert_eq!(completion["fields"]["status"], 200);
}

mod image_host;

use crate::{Config, Service};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use hito_plugin_sdk::{Capability, Manifest};
use hito_testkit::logs::LogCapture;
use http_body_util::BodyExt;
use image_host::{Host, Reply};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

const THUMBNAIL: &[u8] = include_bytes!("../fixtures/hqdefault.jpg");
const VIDEO: &str = "HitoTone001";

struct Harness {
    directory: tempfile::TempDir,
    host: Host,
    app: Router,
    service: Service,
    logs: LogCapture,
    dispatch: tracing::Dispatch,
}

impl Harness {
    async fn start(reply: Reply) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("private-executable-path-sentinel");
        fs::write(
            &executable,
            b"#!/bin/sh\n: > \"${0%/*}/extractor-started\"\nexit 9\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let host = Host::start(reply).await;
        let service = Service::with_media_client(Config { executable }, host.client());
        let logs = LogCapture::unbounded();
        let captured = logs.clone();
        let dispatch = tracing::Dispatch::new(
            tracing_subscriber::registry()
                .with(tracing_subscriber::filter::filter_fn(|metadata| {
                    metadata.target() == "hito_plugin_youtube"
                        || metadata.target().starts_with("hito_plugin_youtube::")
                }))
                .with(tracing_subscriber::filter::LevelFilter::TRACE)
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_writer(move || captured.clone()),
                ),
        );
        let app = service.router();
        Self {
            directory,
            host,
            app,
            service,
            logs,
            dispatch,
        }
    }

    async fn artwork(&self, payload: Value) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::post("/v1/artwork")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer private-bearer-sentinel")
                    .header("cookie", "private-cookie-sentinel=1")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .with_subscriber(self.dispatch.clone())
            .await
            .unwrap()
    }

    fn assert_private(&self) {
        let output = self.logs.text();
        for sentinel in [VIDEO, "ytimg", "hqdefault", "private-"] {
            assert!(!output.contains(sentinel), "logs exposed {sentinel}");
        }
        assert!(!output.contains(self.directory.path().to_str().unwrap()));
        assert!(!Path::new(&self.directory.path().join("extractor-started")).exists());
    }
}

fn recording(id: &str) -> Value {
    json!({"entity": "recording", "provider_id": id})
}

async fn bytes(response: Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

#[tokio::test]
async fn manifest_declares_artwork_alongside_the_existing_capabilities() {
    let harness = Harness::start(Reply::jpeg(THUMBNAIL)).await;
    let response = harness
        .app
        .clone()
        .oneshot(Request::get("/v1/manifest").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let manifest: Manifest = serde_json::from_slice(&bytes(response).await).unwrap();
    manifest.validate().unwrap();
    assert!(manifest.capabilities.contains(&Capability::Artwork));
    assert_eq!(harness.host.requests().len(), 0);
    harness.service.shutdown().await.unwrap();
}

#[tokio::test]
async fn recording_artwork_is_the_fixed_hq_thumbnail_fetched_without_the_extractor() {
    let harness = Harness::start(Reply::jpeg(THUMBNAIL)).await;
    let response = harness.artwork(recording(VIDEO)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/jpeg");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(bytes(response).await, THUMBNAIL);

    let received = harness.host.requests();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].method, "GET");
    assert_eq!(received[0].target, format!("/vi/{VIDEO}/hqdefault.jpg"));
    assert_eq!(received[0].headers["host"], image_host::HOST);
    assert_eq!(received[0].headers["accept-encoding"], "identity");
    for forbidden in ["authorization", "cookie", "range", "referer"] {
        assert!(!received[0].headers.contains_key(forbidden), "{forbidden}");
    }
    harness.assert_private();
    let completion = harness
        .logs
        .text()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|entry| entry["fields"]["event"] == "youtube_artwork_complete")
        .expect("artwork needs a safe completion event");
    assert_eq!(completion["fields"]["outcome"], "complete");
    assert!(completion["fields"]["elapsed_ms"].as_u64().is_some());
    harness.service.shutdown().await.unwrap();
}

#[tokio::test]
async fn upstream_responses_the_host_would_reject_are_refused_after_one_request() {
    let mut oversized_dimensions = THUMBNAIL.to_vec();
    let sof = oversized_dimensions
        .windows(2)
        .position(|pair| pair == [0xff, 0xc0] || pair == [0xff, 0xc2])
        .unwrap();
    oversized_dimensions[sof + 5..sof + 9].copy_from_slice(&[0x08, 0x00, 0x08, 0x00]);
    let mut padded = THUMBNAIL.to_vec();
    padded.truncate(padded.len() - 2);
    padded.resize(2 * 1024 * 1024 + 1, 0);
    let truncated = THUMBNAIL[..THUMBNAIL.len() - 2].to_vec();
    let mut trailing = THUMBNAIL.to_vec();
    trailing.extend_from_slice(b"trailing");
    let cases: Vec<(&str, Reply, StatusCode)> = vec![
        (
            "redirect to another host",
            Reply::raw(
                "302 Found",
                &[("Location", "https://evil.invalid/x.jpg")],
                b"",
            ),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            "missing thumbnail",
            Reply::raw("404 Not Found", &[], b""),
            StatusCode::NOT_FOUND,
        ),
        (
            "html body",
            Reply::raw("200 OK", &[("Content-Type", "text/html")], b"<html>"),
            StatusCode::BAD_GATEWAY,
        ),
        (
            "webp bytes labelled jpeg",
            Reply::jpeg(b"RIFF\x10\0\0\0WEBPVP8 ........"),
            StatusCode::BAD_GATEWAY,
        ),
        (
            "jpeg bytes labelled png",
            Reply::raw("200 OK", &[("Content-Type", "image/png")], THUMBNAIL),
            StatusCode::BAD_GATEWAY,
        ),
        (
            "webp content type",
            Reply::raw("200 OK", &[("Content-Type", "image/webp")], THUMBNAIL),
            StatusCode::BAD_GATEWAY,
        ),
        (
            "gzip encoding",
            Reply::raw(
                "200 OK",
                &[("Content-Type", "image/jpeg"), ("Content-Encoding", "gzip")],
                THUMBNAIL,
            ),
            StatusCode::BAD_GATEWAY,
        ),
        (
            "truncated jpeg",
            Reply::jpeg(&truncated),
            StatusCode::BAD_GATEWAY,
        ),
        (
            "bytes after the end marker",
            Reply::jpeg(&trailing),
            StatusCode::BAD_GATEWAY,
        ),
        (
            "oversized dimensions",
            Reply::jpeg(&oversized_dimensions),
            StatusCode::BAD_GATEWAY,
        ),
        (
            "more than two mebibytes",
            Reply::jpeg(&padded),
            StatusCode::BAD_GATEWAY,
        ),
        (
            "server error",
            Reply::raw("503 Service Unavailable", &[], b""),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ];
    for (name, reply, expected) in cases {
        let harness = Harness::start(reply).await;
        let response = harness.artwork(recording(VIDEO)).await;
        assert_eq!(response.status(), expected, "{name}");
        let body = String::from_utf8_lossy(&bytes(response).await).into_owned();
        assert!(
            !body.contains("evil.invalid") && !body.contains(VIDEO),
            "{name}"
        );
        assert_eq!(harness.host.requests().len(), 1, "{name}");
        harness.assert_private();
        harness.service.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn unsupported_or_malformed_requests_never_reach_the_image_host() {
    let harness = Harness::start(Reply::jpeg(THUMBNAIL)).await;
    for (payload, expected) in [
        (
            json!({"entity": "release", "provider_id": VIDEO}),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({"entity": "playlist", "provider_id": VIDEO}),
            StatusCode::NOT_FOUND,
        ),
        (recording("../../etc/passwd"), StatusCode::BAD_REQUEST),
        (recording("short"), StatusCode::BAD_REQUEST),
        (recording("HitoTone00?"), StatusCode::BAD_REQUEST),
        (
            json!({"entity": "recording", "provider_id": VIDEO, "extra": 1}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = harness.artwork(payload.clone()).await;
        assert_eq!(response.status(), expected, "{payload}");
    }
    assert_eq!(harness.host.requests().len(), 0);
    harness.service.shutdown().await.unwrap();
}

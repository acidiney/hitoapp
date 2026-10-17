use crate::{Config, Service};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_plugin_sdk::{Capability, Manifest};
use http_body_util::BodyExt;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn manifest_declares_implemented_search_and_sources_without_starting_the_executable() {
    let _ambient_no_subscriber_dispatch =
        tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let logs = Capture::default();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(|metadata| {
            metadata.target() == "hito_plugin_youtube"
                || metadata.target().starts_with("hito_plugin_youtube::")
        }))
        .with(tracing_subscriber::filter::LevelFilter::TRACE)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(move || captured.clone()),
        );
    let dispatch = tracing::Dispatch::new(subscriber);
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("private-executable-path-sentinel");
    fs::write(&executable, b"#!/bin/sh\nset -eu\n: > \"${0%/*}/invoked\"\nprintf '%s\\n' 'private-process-output-sentinel'\nexit 7\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let service = Service::new(Config { executable });
    let response = service
        .router()
        .oneshot(
            Request::get("/v1/manifest?private-uri-query-sentinel=private-uri-value-sentinel")
                .header("authorization", "Bearer private-bearer-sentinel")
                .header(
                    "cookie",
                    "private-cookie-sentinel=private-cookie-value-sentinel",
                )
                .header("x-owned-private", "private-header-sentinel")
                .body(Body::empty())
                .unwrap(),
        )
        .with_subscriber(dispatch)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.len() <= 65_536);
    let body = std::str::from_utf8(&bytes).unwrap();
    assert!(
        !body.contains("private-"),
        "Manifest exposed injected private content"
    );
    assert!(
        !body.contains(directory.path().to_str().unwrap()),
        "Manifest exposed a private path"
    );
    let manifest: Manifest = serde_json::from_slice(&bytes).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.id, "org.hito.youtube");
    assert_eq!(manifest.name, "YouTube");
    assert_eq!(manifest.protocol_version, 1);
    assert_eq!(
        manifest.capabilities,
        BTreeSet::from([Capability::Search, Capability::Sources])
    );
    assert!(manifest.authorization.is_none());
    let wire: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(wire.get("authorization").is_none());
    assert!(!directory.path().join("invoked").exists());

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        !output.contains("private-"),
        "Manifest logs exposed injected private content"
    );
    assert!(
        !output.contains(directory.path().to_str().unwrap()),
        "Manifest logs exposed a private path"
    );
    let entries: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let completion = entries
        .iter()
        .find(|entry| entry["fields"]["event"] == "youtube_request_complete")
        .expect("Manifest request needs its safe completion event");
    assert_eq!(completion["level"], "INFO");
    assert_eq!(completion["fields"]["status"], 200);
    assert!(completion["fields"]["elapsed_ms"].as_u64().is_some());
    let span = completion["spans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|span| span["name"] == "youtube_request")
        .expect("Manifest request needs its correlation span");
    assert_eq!(span["operation"], "manifest");
    assert!(span["request_id"].as_u64().is_some());
    assert!(
        entries
            .iter()
            .all(|entry| entry["fields"]["event"] != "youtube_search_complete")
    );
    service.shutdown().await.unwrap();
}

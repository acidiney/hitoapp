mod support;

use axum::{body::Body, http::Request};
use hito_plugin_lrclib::{Config, router};
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
    time::Duration,
};
use support::{TOKEN, Upstream, record};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;
use url::Url;

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, value: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(value);
        Ok(value.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn request(path: &str, payload: Value) -> Request<Body> {
    Request::post(format!("{path}?private-query=owned-private-query-marker"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("x-hito-plugin-protocol", "1")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap()
}

#[tokio::test]
async fn actual_router_logs_named_workflows_cancellation_and_safe_correlations() {
    let (upstream, server) = Upstream::start(record(41)).await;
    let app = router(
        Config::new(TOKEN)
            .unwrap()
            .with_endpoint(Url::parse(&server.url).unwrap())
            .unwrap(),
    )
    .unwrap();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writer = Capture(bytes.clone());
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(
                "hito_plugin_lrclib=trace,hito_plugin_sdk=trace",
            ))
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_ansi(false)
                    .with_writer(move || writer.clone()),
            ),
    );
    let response = app
        .clone()
        .oneshot(request(
            "/v1/recordings/metadata",
            json!({"recording_id":"41"}),
        ))
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    {
        let pending = app
            .clone()
            .oneshot(request(
                "/v2/lyrics",
                json!({"recording":{"id":"41","revision":null}}),
            ))
            .with_subscriber(dispatch.clone());
        tokio::pin!(pending);
        tokio::select! {
            _ = &mut pending => panic!("cooldown should still hold the request"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {},
        }
    }
    assert_eq!(upstream.seen.lock().unwrap().len(), 1);
    let response = app
        .oneshot(request(
            "/v2/lyrics",
            json!({"recording":{"id":"41","revision":null}}),
        ))
        .with_subscriber(dispatch)
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let logs = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    let events: Vec<Value> = logs
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|entry| entry["fields"]["event"] == "lrclib_plugin_request_finished")
        .collect();
    assert_eq!(events.len(), 3);
    let mut ids = Vec::new();
    for (event, (operation, outcome)) in events.iter().zip([
        ("metadata", "success"),
        ("lyrics", "cancelled"),
        ("lyrics", "success"),
    ]) {
        assert_eq!(event["span"]["operation"], operation);
        assert_eq!(event["fields"]["outcome"], outcome);
        assert!(event["fields"]["elapsed_ms"].is_u64());
        let id = event["span"]["request_id"].as_str().unwrap();
        assert!(uuid::Uuid::parse_str(id).is_ok());
        ids.push(id);
    }
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 3);
    for private in [
        TOKEN,
        "Owned river",
        "Fixture ensemble",
        "Synthetic river",
        "Owned horizon",
        "owned-private-query-marker",
        &server.url,
    ] {
        assert!(
            !logs.contains(private),
            "private provider data entered workflow logs"
        );
    }
    server.close().await;
}

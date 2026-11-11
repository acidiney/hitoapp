#[path = "logging/upstream.rs"]
mod protocol_upstream;
mod support;

use protocol_upstream::ProtocolUpstream;

use axum::{body::Body, http::Request};
use hito_plugin_lrclib::{Config, router};
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
    time::Duration,
};
use support::{Reply, TOKEN, Upstream, record};
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

fn info_capture() -> (Arc<Mutex<Vec<u8>>>, tracing::Dispatch) {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writer = Capture(bytes.clone());
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new("info"))
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_ansi(false)
                    .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
                    .with_writer(move || writer.clone()),
            ),
    );
    (bytes, dispatch)
}

fn captured(bytes: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8(bytes.lock().unwrap().clone()).unwrap()
}

fn failure(logs: &str, reason: &str, stage: &str, status: Option<u16>, operation: &str) -> Value {
    let events: Vec<Value> = logs
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        events.len(),
        1,
        "one actionable warning per upstream failure"
    );
    let event = &events[0];
    assert_eq!(event["level"], "WARN");
    let blocked = matches!(reason, "invalid_retry_after" | "retry_after_overflow");
    assert_eq!(
        event["fields"]["event"],
        if blocked {
            "lrclib_upstream_cooldown_blocked"
        } else {
            "lrclib_upstream_failed"
        }
    );
    assert_eq!(
        event["fields"]["recovery"],
        if blocked {
            json!("restart_service")
        } else {
            Value::Null
        }
    );
    assert_eq!(event["fields"]["reason"], reason);
    assert_eq!(event["fields"]["stage"], stage);
    assert_eq!(event["fields"]["upstream_status"], json!(status));
    assert!(event["fields"]["elapsed_ms"].is_u64());
    assert_eq!(event["fields"]["operation"], operation);
    assert!(uuid::Uuid::parse_str(event["fields"]["failure_id"].as_str().unwrap()).is_ok());
    event.clone()
}

async fn failed_reply(value: Value, reply: Option<Reply>, route: &str, payload: Value) -> String {
    let (upstream, server) = Upstream::start(value).await;
    *upstream.response.lock().unwrap() = reply;
    let app = router(
        Config::new(TOKEN)
            .unwrap()
            .with_contact("owned-private-contact@example.invalid")
            .unwrap()
            .with_endpoint(Url::parse(&server.url).unwrap())
            .unwrap(),
    )
    .unwrap();
    let (bytes, dispatch) = info_capture();
    let response = app
        .oneshot(request(route, payload))
        .with_subscriber(dispatch)
        .await
        .unwrap();
    assert_eq!(response.status(), 502);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(upstream.seen.lock().unwrap().len(), 1);
    let logs = captured(&bytes);
    for private in [
        TOKEN,
        "Owned river",
        "Fixture ensemble",
        "Synthetic river",
        "Owned horizon",
        "owned-private-query-marker",
        "owned-private-search-marker",
        "owned-private-body-marker",
        "owned-private-contact@example.invalid",
        "/api/search",
        &server.url,
    ] {
        assert!(!logs.contains(private), "private data entered failure logs");
    }
    server.close().await;
    logs
}

#[tokio::test]
async fn malformed_upstream_json_has_safe_actionable_diagnostics_at_info() {
    let logs = failed_reply(
        record(41),
        Some(Reply::Typed(
            "application/json",
            b"{owned-private-body-marker".to_vec(),
        )),
        "/v1/search",
        json!({"query":"owned-private-search-marker","cursor":null,"limit":20}),
    )
    .await;
    failure(&logs, "invalid_json", "decode", Some(200), "search");
}

#[tokio::test]
async fn semantic_upstream_rejections_are_reported_once_at_info_for_each_workflow() {
    for (route, payload, operation) in [
        (
            "/v1/search",
            json!({"query":"owned-private-search-marker","cursor":null,"limit":20}),
            "search",
        ),
        (
            "/v1/recordings/detail",
            json!({"recording_id":"41"}),
            "detail",
        ),
        (
            "/v1/recordings/metadata",
            json!({"recording_id":"41"}),
            "metadata",
        ),
        (
            "/v2/lyrics",
            json!({"recording":{"id":"41","revision":null}}),
            "lyrics",
        ),
    ] {
        let mut value = record(41);
        value["trackName"] = json!("Owned private invalid title\t");
        if operation == "search" {
            value = json!([value]);
        }
        let logs = failed_reply(value, None, route, payload).await;
        failure(&logs, "invalid_record", "validation", None, operation);
        assert!(!logs.contains("Owned private invalid title"));
    }
}

async fn failed_protocol(reply: Vec<u8>, status: u16) -> String {
    let server = ProtocolUpstream::start(reply, false).await;
    let app = router(
        Config::new(TOKEN)
            .unwrap()
            .with_endpoint(Url::parse(&server.url).unwrap())
            .unwrap(),
    )
    .unwrap();
    let (bytes, dispatch) = info_capture();
    let response = app
        .oneshot(request(
            "/v1/recordings/metadata",
            json!({"recording_id":"41"}),
        ))
        .with_subscriber(dispatch)
        .await
        .unwrap();
    assert_eq!(response.status(), status);
    let logs = captured(&bytes);
    for private in [
        TOKEN,
        "owned-private-query-marker",
        "owned-private-body-marker",
        &server.url,
    ] {
        assert!(
            !logs.contains(private),
            "private data entered protocol failure logs"
        );
    }
    server.close().await;
    logs
}

#[tokio::test]
async fn invalid_upstream_headers_and_body_limits_have_distinct_safe_diagnostics() {
    let maximum = hito_plugin_sdk::MAX_LYRICS_RESPONSE_BYTES;
    let cases = [
        ("Content-Type: text/html\r\n".to_owned(), b"owned-private-body-marker".to_vec(), "invalid_content_type", "headers"),
        ("Content-Type: application/json\r\nContent-Type: application/json\r\n".to_owned(), vec![], "invalid_content_type", "headers"),
        ("Content-Type: application/json\r\nContent-Encoding: gzip\r\n".to_owned(), vec![], "unsupported_content_encoding", "headers"),
        ("Content-Type: application/json\r\nContent-Encoding: identity\r\nContent-Encoding: identity\r\n".to_owned(), vec![], "unsupported_content_encoding", "headers"),
        (format!("Content-Type: application/json\r\n{}", (0..33).map(|n| format!("x-owned-{n}: owned-private-header-marker\r\n")).collect::<String>()), vec![], "too_many_headers", "headers"),
        (format!("Content-Type: application/json\r\nContent-Length: {}\r\n", maximum + 1), vec![], "announced_body_limit", "headers"),
        ("Content-Type: application/json\r\n".to_owned(), vec![b'x'; maximum + 1], "streamed_body_limit", "body"),
    ];
    for (headers, body, reason, stage) in cases {
        let mut reply =
            format!("HTTP/1.1 200 OK\r\n{headers}Connection: close\r\n\r\n").into_bytes();
        reply.extend_from_slice(&body);
        let logs = failed_protocol(reply, 502).await;
        failure(&logs, reason, stage, Some(200), "metadata");
        assert!(!logs.contains("owned-private-header-marker"));
    }
}

#[tokio::test]
async fn upstream_http_failures_report_status_without_response_content() {
    for status in [302, 500] {
        let logs = failed_protocol(
            format!("HTTP/1.1 {status} Owned failure\r\nConnection: close\r\n\r\nowned-private-body-marker").into_bytes(),
            503,
        ).await;
        failure(&logs, "http_status", "response", Some(status), "metadata");
    }
}

#[tokio::test]
async fn upstream_transport_failures_distinguish_response_admission_from_body_reads() {
    for (reply, stage, status) in [
        (Vec::new(), "request", None),
        (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n{owned-private-body-marker".to_vec(), "body", Some(200)),
    ] {
        let logs = failed_protocol(reply, 503).await;
        failure(&logs, "transport", stage, status, "metadata");
    }
}

#[tokio::test]
async fn upstream_deadlines_produce_one_timed_warning_and_retire_the_response() {
    for reply in [
        Vec::new(),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n{owned-private-body-marker".to_vec(),
    ] {
        let server = ProtocolUpstream::start(reply, true).await;
        let app = router(Config::new(TOKEN).unwrap()
            .with_endpoint(Url::parse(&server.url).unwrap()).unwrap()).unwrap();
        let (bytes, dispatch) = info_capture();
        let response = tokio::time::timeout(Duration::from_secs(8), app.oneshot(request(
            "/v1/recordings/metadata", json!({"recording_id":"41"}),
        )).with_subscriber(dispatch)).await.unwrap().unwrap();
        assert_eq!(response.status(), 503);
        let logs = captured(&bytes);
        let event = failure(&logs, "deadline_exceeded", "exchange", None, "metadata");
        assert!(event["fields"]["elapsed_ms"].as_u64().unwrap() >= 5000);
        assert!(!logs.contains(&server.url));
        assert!(!logs.contains("owned-private-body-marker"));
        server.close().await;
    }
}

#[tokio::test]
async fn upstream_cooldowns_warn_once_and_do_not_flood_busy_polling() {
    for status in [
        axum::http::StatusCode::TOO_MANY_REQUESTS,
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
    ] {
        for (retry_after, reason) in [
            ("600", "cooldown"),
            ("owned-private-retry-after-marker", "invalid_retry_after"),
            ("18446744073709551615", "retry_after_overflow"),
        ] {
            let (upstream, server) = Upstream::start(record(41)).await;
            *upstream.response.lock().unwrap() =
                Some(Reply::Retry(status, vec![retry_after.to_owned()]));
            let app = router(
                Config::new(TOKEN)
                    .unwrap()
                    .with_endpoint(Url::parse(&server.url).unwrap())
                    .unwrap(),
            )
            .unwrap();
            let (bytes, dispatch) = info_capture();
            for _ in 0..6 {
                let response = app
                    .clone()
                    .oneshot(request(
                        "/v2/lyrics",
                        json!({"recording":{"id":"41","revision":null}}),
                    ))
                    .with_subscriber(dispatch.clone())
                    .await
                    .unwrap();
                assert_eq!(response.status(), 503);
                use http_body_util::BodyExt;
                let body = response.into_body().collect().await.unwrap().to_bytes();
                assert_eq!(
                    serde_json::from_slice::<Value>(&body).unwrap(),
                    json!({"error":"The lyric provider is busy. Try again later."})
                );
            }
            assert_eq!(upstream.seen.lock().unwrap().len(), 1);
            let logs = captured(&bytes);
            failure(&logs, reason, "cooldown", Some(status.as_u16()), "lyrics");
            assert!(!logs.contains("owned-private-retry-after-marker"));
            assert!(!logs.contains("owned-private-query-marker"));
            assert!(!logs.contains(TOKEN));
            assert!(!logs.contains(&server.url));
            server.close().await;
        }
    }
}

#[tokio::test]
async fn success_not_found_local_rejections_busy_and_cancelled_work_stay_quiet_at_info() {
    let (upstream, server) = Upstream::start(record(41)).await;
    let app = router(
        Config::new(TOKEN)
            .unwrap()
            .with_endpoint(Url::parse(&server.url).unwrap())
            .unwrap(),
    )
    .unwrap();
    let (bytes, dispatch) = info_capture();
    let metadata = || request("/v1/recordings/metadata", json!({"recording_id":"41"}));
    assert_eq!(
        app.clone()
            .oneshot(metadata())
            .with_subscriber(dispatch.clone())
            .await
            .unwrap()
            .status(),
        200
    );
    {
        let pending = app
            .clone()
            .oneshot(metadata())
            .with_subscriber(dispatch.clone());
        tokio::pin!(pending);
        tokio::select! {
            _ = &mut pending => panic!("owned pacing waiter completed before cancellation"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {},
        }
        assert_eq!(
            app.clone()
                .oneshot(metadata())
                .with_subscriber(dispatch.clone())
                .await
                .unwrap()
                .status(),
            503
        );
    }
    *upstream.response.lock().unwrap() = Some(Reply::Status(axum::http::StatusCode::NOT_FOUND));
    assert_eq!(
        app.clone()
            .oneshot(metadata())
            .with_subscriber(dispatch.clone())
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "/v2/lyrics",
                json!({"recording":{"id":"41","revision":null}})
            ))
            .with_subscriber(dispatch.clone())
            .await
            .unwrap()
            .status(),
        200
    );
    let mut unauthorized = metadata();
    unauthorized.headers_mut().remove("authorization");
    assert_eq!(
        app.clone()
            .oneshot(unauthorized)
            .with_subscriber(dispatch.clone())
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        app.oneshot(request(
            "/v1/recordings/metadata",
            json!({"recording_id":"../owned-private-id"})
        ))
        .with_subscriber(dispatch.clone())
        .await
        .unwrap()
        .status(),
        400
    );
    assert_eq!(upstream.seen.lock().unwrap().len(), 3);
    assert!(
        captured(&bytes).is_empty(),
        "routine traffic emitted info logs"
    );
    server.close().await;

    let mut held = ProtocolUpstream::start(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n{owned-private-body-marker".to_vec(), true).await;
    let app = router(
        Config::new(TOKEN)
            .unwrap()
            .with_endpoint(Url::parse(&held.url).unwrap())
            .unwrap(),
    )
    .unwrap();
    {
        let pending = app
            .clone()
            .oneshot(metadata())
            .with_subscriber(dispatch.clone());
        tokio::pin!(pending);
        tokio::select! {
            _ = &mut pending => panic!("owned held body completed before cancellation"),
            started = held.started.take().unwrap() => started.unwrap(),
        }
        assert_eq!(
            app.oneshot(metadata())
                .with_subscriber(dispatch)
                .await
                .unwrap()
                .status(),
            503
        );
    }
    held.close().await;
    assert!(
        captured(&bytes).is_empty(),
        "cancellation or concurrency emitted info logs"
    );
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
        .clone()
        .oneshot(request(
            "/v2/lyrics",
            json!({"recording":{"id":"41","revision":null}}),
        ))
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    *upstream.response.lock().unwrap() = Some(Reply::Typed("application/json", b"{".to_vec()));
    assert_eq!(
        app.oneshot(request(
            "/v1/recordings/metadata",
            json!({"recording_id":"41"})
        ))
        .with_subscriber(dispatch)
        .await
        .unwrap()
        .status(),
        502
    );
    let logs = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    let events: Vec<Value> = logs
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|entry| entry["fields"]["event"] == "lrclib_plugin_request_finished")
        .collect();
    assert_eq!(events.len(), 4);
    let mut ids = Vec::new();
    for (event, (operation, outcome)) in events.iter().zip([
        ("metadata", "success"),
        ("lyrics", "cancelled"),
        ("lyrics", "success"),
        ("metadata", "rejected"),
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
    assert_eq!(ids.len(), 4);
    let warning: Value = logs
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|entry| entry["fields"]["event"] == "lrclib_upstream_failed")
        .unwrap();
    let request_span = warning["spans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|span| span["name"] == "lrclib_plugin_request")
        .unwrap();
    assert_eq!(request_span["request_id"], events[3]["span"]["request_id"]);
    assert_eq!(request_span["operation"], "metadata");
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

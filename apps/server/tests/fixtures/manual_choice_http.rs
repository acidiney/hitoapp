use super::{fixture::Provider, http};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};

pub(super) async fn connect(address: SocketAddr, token: &str, provider: &Provider) -> Value {
    let service = call(
        address,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Owned manual source provider", "base_url": provider.url}),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    call(
        address,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        Some(token),
        StatusCode::CREATED,
    )
    .await
}

pub(super) async fn ready_download(
    address: SocketAddr,
    token: &str,
    path: &str,
    receipt: &Value,
) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let response = http::call(address, "GET", path, None, Some(token)).await;
        assert_eq!(response.status, StatusCode::OK);
        let status = response.json();
        assert!(
            status["intent_id"] == receipt["intent_id"],
            "status must belong to the current explicit download request"
        );
        if status["status"] == "ready" {
            assert!(status["source_id"].is_string());
            return status;
        }
        assert!(
            matches!(
                status["status"].as_str(),
                Some("queued" | "running" | "verifying")
            ),
            "owned download must not fail"
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "owned deterministic download must settle within the test deadline"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub(super) fn source_named(sources: &Value, label: &str) -> String {
    sources
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["label"].as_str() == Some(label))
        .expect("the owned provider source must be listed")["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

pub(super) fn assert_choice(choice: &Value, recording: &str, request: &Value) {
    let mut keys: Vec<&str> = choice
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["recording_id", "request_id", "revision", "source_id"]
    );
    assert!(
        choice["request_id"] == request["request_id"]
            && choice["source_id"] == request["source_id"]
            && choice["recording_id"] == recording
    );
    assert!(
        choice["revision"]
            .as_u64()
            .is_some_and(|revision| revision > 0)
    );
}

pub(super) async fn assert_saved_selection(
    address: SocketAddr,
    token: &str,
    sources_path: &str,
    saved: &str,
    a: &str,
) {
    let sources = http::get(address, token, sources_path).await;
    let ready: Vec<&Value> = sources
        .as_array()
        .unwrap()
        .iter()
        .filter(|source| source["ready"] == true)
        .collect();
    assert_eq!(ready.len(), 1);
    assert!(
        ready[0]["id"] == saved && ready[0]["provider"] == "download",
        "only the selected binding's verified saved copy is eligible"
    );
    assert!(
        sources
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["id"] == a && source["ready"] == false),
        "active A must remain retained and ineligible"
    );
}

pub(super) async fn assert_audio(address: SocketAddr, token: &str, source: &str, expected: &[u8]) {
    let response = http::call(
        address,
        "GET",
        &format!("/api/v1/sources/{source}/audio"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(
        response.body == expected,
        "actual relayed bytes must belong to the explicitly chosen representation"
    );
}

pub(super) async fn assert_denied_audio(address: SocketAddr, token: &str, source: &str) {
    let response = http::call(
        address,
        "GET",
        &format!("/api/v1/sources/{source}/audio"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "an ineligible representation must not bypass the choice through a known Hito source URL"
    );
}

pub(super) async fn call(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
    expected: StatusCode,
) -> Value {
    let response = http::call(address, method, path, Some(body), token).await;
    assert_eq!(response.status, expected);
    if expected == StatusCode::NO_CONTENT {
        Value::Null
    } else {
        response.json()
    }
}

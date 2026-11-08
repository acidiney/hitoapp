#[allow(dead_code)]
#[path = "../fixtures/source_resolution.rs"]
mod fixture;
#[path = "../fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "../fixtures/artwork_http.rs"]
mod http;

use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe};

#[tokio::test]
async fn a_source_chosen_from_the_provider_ranking_is_labelled_a_best_guess() {
    let directory = tempfile::tempdir().unwrap();
    let services = host::open(directory.path(), "rank-fallback-host").await;
    let mut server = host::Host::start(services).await;
    let mut provider = fixture::Provider::start_without_exact_match().await;
    let outcome = AssertUnwindSafe(exercise(server.address, &provider))
        .catch_unwind()
        .await;
    server.request_shutdown();
    let joined = server.join().await;
    let provider_joined = provider.finish().await;
    assert!(joined && provider_joined, "owned servers must finish");
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(address: SocketAddr, provider: &fixture::Provider) {
    let session = call(
        address,
        "/api/v1/setup",
        json!({"username": "owner", "password": "owned rank fallback password"}),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let service = call(
        address,
        "/api/v1/plugins/services",
        json!({"name": "Owned fallback service", "base_url": provider.url}),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    let installation = call(address, "/api/v1/plugins", json!({"service_id": service["id"], "service_revision": service["revision"], "bearer_token": null}), Some(token), StatusCode::CREATED).await;
    let original = call(address, &format!("/api/v1/plugins/{}/recordings", installation["id"].as_str().unwrap()), json!({"installation_revision": installation["revision"], "provider_recording_id": fixture::TARGET}), Some(token), StatusCode::OK).await;
    let recording = original["id"].as_str().unwrap();
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    let result = call(
        address,
        &format!("{sources_path}/resolve"),
        json!({"request_id": "0d3f5b7c-62c1-4c38-8c2e-1a7f2c6a9b11"}),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_eq!(result["outcome"], "ready");
    assert_eq!(result["sources"][0]["match_kind"], "best_guess");
    let listed = http::get(address, token, &sources_path).await;
    assert_eq!(listed[0]["match_kind"], "best_guess");
    assert_eq!(listed[0]["label"], "Owned remix audio");
}

async fn call(
    address: SocketAddr,
    path: &str,
    body: Value,
    token: Option<&str>,
    expected: StatusCode,
) -> Value {
    let response = http::call(address, "POST", path, Some(body), token).await;
    assert_eq!(response.status, expected);
    response.json()
}

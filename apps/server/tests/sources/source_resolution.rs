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
async fn automatic_resolution_retains_recording_identity_and_replays_a_committed_result() {
    let directory = tempfile::tempdir().unwrap();
    let services = host::open(directory.path(), "automatic-source-host").await;
    let mut server = host::Host::start(services).await;
    let mut provider = fixture::Provider::start().await;
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
        "POST",
        "/api/v1/setup",
        json!({"username": "owner", "password": "owned automatic source password"}),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let service = call(
        address,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Owned automatic service", "base_url": provider.url}),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    let installation = call(address, "POST", "/api/v1/plugins", json!({"service_id": service["id"], "service_revision": service["revision"], "bearer_token": null}), Some(token), StatusCode::CREATED).await;
    let original = call(address, "POST", &format!("/api/v1/plugins/{}/recordings", installation["id"].as_str().unwrap()), json!({"installation_revision": installation["revision"], "provider_recording_id": fixture::TARGET}), Some(token), StatusCode::OK).await;
    assert_eq!(original["available"], false);
    let recording = original["id"].as_str().unwrap();
    let metadata_path = format!("/api/v1/recordings/{recording}/metadata");
    let before_metadata = http::get(address, token, &metadata_path).await;
    let before = provider.counts();
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    let unavailable = http::get(address, token, &sources_path).await;
    assert!(unavailable.as_array().unwrap().is_empty());
    assert_eq!(provider.counts(), before, "source display must not search");
    let resolve = format!("{sources_path}/resolve");
    let request_id = "610ca375-1b41-460f-bafb-265e714356d2";
    let body = json!({"request_id": request_id});
    let result = call(
        address,
        "POST",
        &resolve,
        body.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_eq!(result["request_id"], request_id);
    assert_eq!(result["recording_id"], recording);
    assert_eq!(result["outcome"], "ready");
    assert!(uuid::Uuid::parse_str(result["resolution_id"].as_str().unwrap()).is_ok());
    let sources = result["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["recording_id"], recording);
    assert_eq!(sources[0]["ready"], true);
    assert_eq!(sources[0]["match_kind"], "verified");
    assert!(sources[0]["label"].as_str() == Some("Owned studio audio"));
    assert_eq!(
        provider.counts(),
        (1, 2, 0),
        "only the suitable studio candidate needs details"
    );
    let replay = call(address, "POST", &resolve, body, Some(token), StatusCode::OK).await;
    assert!(
        replay == result,
        "replayed receipt must preserve the committed identities"
    );
    assert_eq!(
        provider.counts(),
        (1, 2, 0),
        "receipt replay must not contact providers"
    );
    let after_metadata = http::get(address, token, &metadata_path).await;
    assert!(before_metadata["selected_observation"]["id"].is_string());
    assert!(
        after_metadata["selected_observation"] == before_metadata["selected_observation"],
        "automatic resolution must preserve the primary selected observation"
    );
    assert!(
        after_metadata["title"] == before_metadata["title"],
        "display title must remain unchanged"
    );
    assert!(
        after_metadata["artist"] == before_metadata["artist"],
        "display artist must remain unchanged"
    );
    let recordings = http::get(address, token, "/api/v1/recordings").await;
    assert_eq!(
        recordings["items"].as_array().unwrap().len(),
        1,
        "resolution must not adopt another recording"
    );
    assert_eq!(recordings["items"][0]["id"], recording);
    assert_eq!(recordings["items"][0]["available"], true);
    let encoded = result.to_string();
    for private in [fixture::TARGET, fixture::CHOSEN, provider.url.as_str()] {
        assert!(
            !encoded.contains(private),
            "resolution must omit opaque provider identities and URLs"
        );
    }
}

async fn call(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
    expected: StatusCode,
) -> Value {
    let response = http::call(address, method, path, Some(body), token).await;
    assert_eq!(response.status, expected);
    response.json()
}

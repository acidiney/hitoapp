#[path = "../fixtures/source_preparation_metadata.rs"]
mod fixture;
#[path = "../fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "../fixtures/artwork_http.rs"]
mod http;

use axum::http::StatusCode;
use fixture::Provider;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe};

#[tokio::test]
async fn source_labels_and_preparation_actions_follow_current_account_authority() {
    let directory = tempfile::tempdir().unwrap();
    let services = host::open(directory.path(), "preparation-metadata-host").await;
    let mut host = host::Host::start(services).await;
    let mut capable = Provider::start(true).await;
    let mut sources_only = Provider::start(false).await;
    let outcome = AssertUnwindSafe(exercise(host.address, &capable, &sources_only))
        .catch_unwind()
        .await;
    host.request_shutdown();
    let host_joined = host.join().await;
    let capable_joined = capable.finish().await;
    let sources_only_joined = sources_only.finish().await;
    assert!(
        host_joined && capable_joined && sources_only_joined,
        "the real host and both owned protocol providers must join"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(address: SocketAddr, capable: &Provider, sources_only: &Provider) {
    let session = request(
        address,
        "POST",
        "/api/v1/setup",
        json!({"username": "owner", "password": "owned metadata owner password"}),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let (service, recording) = adopt(address, token, capable).await;
    let path = format!(
        "/api/v1/recordings/{}/sources",
        recording["id"].as_str().unwrap()
    );
    let before = sources(address, token, &path, capable).await;
    assert_eq!(before.as_array().unwrap().len(), 2);
    let unready = before
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["ready"] == false)
        .unwrap();
    assert_source(unready, fixture::ORIGINAL_LABEL, true);
    let source = unready["id"].as_str().unwrap();
    let ready = before
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["ready"] == true)
        .unwrap();
    assert_source(ready, fixture::READY_LABEL, false);

    let (_, other_recording) = adopt(address, token, sources_only).await;
    let other_path = format!(
        "/api/v1/recordings/{}/sources",
        other_recording["id"].as_str().unwrap()
    );
    let other = sources(address, token, &other_path, sources_only).await;
    assert_eq!(other.as_array().unwrap().len(), 2);
    for source in other.as_array().unwrap() {
        let label = if source["ready"] == true {
            fixture::READY_LABEL
        } else {
            fixture::ORIGINAL_LABEL
        };
        assert_source(source, label, false);
    }

    let account = json!({"username": "listener", "password": "owned metadata listener password"});
    let listener = request(
        address,
        "POST",
        "/api/v1/users",
        account.clone(),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    let session = request(
        address,
        "POST",
        "/api/v1/login",
        account,
        None,
        StatusCode::OK,
    )
    .await;
    let listener_token = session["token"].as_str().unwrap();
    let access = format!(
        "/api/v1/libraries/{}/members/{}",
        recording["library_id"].as_str().unwrap(),
        listener["id"].as_str().unwrap()
    );
    let counts = (capable.requests(), sources_only.requests());
    request(
        address,
        "PUT",
        &access,
        Value::Null,
        Some(token),
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    let listener_sources = request(
        address,
        "GET",
        &path,
        Value::Null,
        Some(listener_token),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_private_sources(&listener_sources, capable);
    let response = http::call(
        address,
        "POST",
        &format!("{path}/{source}/preparation"),
        Some(json!({"request_id": "f56bab5b-19bb-4c2c-bdda-f751493b973c"})),
        Some(listener_token),
    )
    .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!((capable.requests(), sources_only.requests()), counts);

    capable.snapshot(1);
    let refreshed = request(
        address,
        "POST",
        &format!("{path}/refresh"),
        Value::Null,
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_private_sources(&refreshed, capable);
    let updated = refreshed
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == source)
        .unwrap();
    assert_source(updated, fixture::UPDATED_LABEL, true);

    capable.snapshot(2);
    let replaced = request(
        address,
        "POST",
        &format!("{path}/refresh"),
        Value::Null,
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_private_sources(&replaced, capable);
    assert_eq!(replaced.as_array().unwrap().len(), 1);
    assert!(
        replaced
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != source)
    );
    assert_source(&replaced[0], fixture::READY_LABEL, false);

    capable.snapshot(0);
    let restored = request(
        address,
        "POST",
        &format!("{path}/refresh"),
        Value::Null,
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_private_sources(&restored, capable);
    assert!(
        restored
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == source && item["can_prepare"] == true)
    );
    request(
        address,
        "DELETE",
        &format!(
            "/api/v1/plugins/services/{}",
            service["id"].as_str().unwrap()
        ),
        json!({"revision": service["revision"]}),
        Some(token),
        StatusCode::NO_CONTENT,
    )
    .await;
    let revoked = sources(address, token, &path, capable).await;
    assert!(
        revoked
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["can_prepare"] == false)
    );
    assert_eq!(capable.unexpected(), 0);
    assert_eq!(sources_only.unexpected(), 0);
}

async fn adopt(address: SocketAddr, token: &str, provider: &Provider) -> (Value, Value) {
    let service = request(
        address,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Owned metadata provider", "base_url": provider.url}),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    let installation = request(
        address,
        "POST",
        "/api/v1/plugins",
        json!({
            "service_id": service["id"], "service_revision": service["revision"],
            "bearer_token": fixture::TOKEN
        }),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    let recording = request(
        address,
        "POST",
        &format!(
            "/api/v1/plugins/{}/recordings",
            installation["id"].as_str().unwrap()
        ),
        json!({
            "installation_revision": installation["revision"],
            "provider_recording_id": fixture::RECORDING
        }),
        Some(token),
        StatusCode::OK,
    )
    .await;
    (service, recording)
}

async fn sources(address: SocketAddr, token: &str, path: &str, provider: &Provider) -> Value {
    let before = provider.requests();
    let published = request(
        address,
        "GET",
        path,
        Value::Null,
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_private_sources(&published, provider);
    assert_eq!(
        provider.requests(),
        before,
        "source display must not call the provider"
    );
    published
}

fn assert_source(source: &Value, label: &str, can_prepare: bool) {
    assert!(
        source["label"].as_str() == Some(label),
        "the source must expose its current human label"
    );
    assert!(
        source["can_prepare"].as_bool() == Some(can_prepare),
        "the source action must follow approved current authority"
    );
    let mut keys = source
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "can_prepare",
            "device_id",
            "id",
            "label",
            "provider",
            "ready",
            "recording_id"
        ]
    );
}

fn assert_private_sources(sources: &Value, provider: &Provider) {
    let encoded = sources.to_string();
    for private in [
        fixture::TOKEN,
        fixture::RECORDING,
        fixture::UNREADY,
        fixture::READY,
        provider.url.as_str(),
    ] {
        assert!(
            !encoded.contains(private),
            "source metadata must omit private provider data"
        );
    }
}

async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
    expected: StatusCode,
) -> Value {
    let response = http::call(address, method, path, Some(body), token).await;
    assert_eq!(response.status, expected);
    if response.status == StatusCode::NO_CONTENT {
        Value::Null
    } else {
        response.json()
    }
}

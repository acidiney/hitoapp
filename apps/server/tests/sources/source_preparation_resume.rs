#[path = "../fixtures/source_preparation_resume.rs"]
mod fixture;
#[path = "../fixtures/download_host.rs"]
mod host;
#[path = "../fixtures/artwork_http.rs"]
mod http;
#[allow(dead_code)]
#[path = "../api/support.rs"]
mod support;

use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmitResponse,
};
use host::Host;
use serde_json::{Value, json};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{net::SocketAddr, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn an_accepted_preparation_resumes_the_same_handle_after_two_days_offline() {
    let directory = tempfile::tempdir().unwrap();
    let audio = support::write_wav(&directory.path().join("owned-offline.wav"));
    let mut provider = fixture::Provider::start(audio.clone()).await;
    let mut hosts = Vec::new();
    let outcome = AssertUnwindSafe(exercise(directory.path(), &provider, &audio, &mut hosts))
        .catch_unwind()
        .await;
    for host in &mut hosts {
        host.request_shutdown();
    }
    let mut joined = true;
    for host in &mut hosts {
        joined &= host.join().await;
    }
    joined &= provider.finish().await;
    assert!(
        joined,
        "all real hosts and the owned provider must join on success or panic"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(
    directory: &Path,
    provider: &fixture::Provider,
    audio: &[u8],
    hosts: &mut Vec<Host>,
) {
    let services = host::open(directory, "preparation-offline").await;
    hosts.push(Host::start(services.clone()).await);
    let address = hosts[0].address;
    let response = http::call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({
            "username": "owner", "password": "owned offline preparation password"
        })),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let session = response.json();
    let token = session["token"].as_str().unwrap();
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins/services",
        Some(json!({
            "name": "Owned offline provider", "base_url": provider.url
        })),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let service = response.json();
    let response = http::call(address, "POST", "/api/v1/plugins", Some(json!({
        "service_id": service["id"], "service_revision": service["revision"], "bearer_token": fixture::TOKEN
    })), Some(token)).await;
    assert_eq!(response.status, StatusCode::CREATED);
    let installation = response.json();
    let response = http::call(address, "POST", &format!("/api/v1/plugins/{}/recordings", installation["id"].as_str().unwrap()),
        Some(json!({"installation_revision": installation["revision"], "provider_recording_id": fixture::RECORDING})), Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    let recording = response.json();
    assert_eq!(recording["available"], false);
    let recording = recording["id"].as_str().unwrap();
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    let before = http::get(address, token, &sources_path).await;
    assert_eq!(before.as_array().unwrap().len(), 1);
    assert_eq!(before[0]["ready"], false);
    let source = before[0]["id"].as_str().unwrap();
    let preparation_path = format!("{sources_path}/{source}/preparation");
    let download_path = format!("/api/v1/recordings/{recording}/download");
    let settings = http::get(address, token, "/api/v1/plugins").await;
    let request = json!({"request_id": "0b818ebd-5875-4cf3-b31f-8c5a8416ff36"});
    let response = http::call(
        address,
        "POST",
        &preparation_path,
        Some(request.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let receipt = response.json();
    let operation = receipt["preparation_id"].as_str().unwrap();
    assert!(!Uuid::parse_str(operation).unwrap().is_nil());
    assert_eq!(receipt["recording_id"], recording);
    assert_eq!(receipt["source_id"], source);
    let begin = tokio::time::timeout(Duration::from_secs(3), provider.next_begin())
        .await
        .expect("the real host must submit the original durable operation once");
    assert_headers(&begin.headers);
    assert_eq!(
        begin.body,
        json!({"operation_id": operation, "recording_id": fixture::RECORDING, "source_id": fixture::SOURCE})
    );
    let begin_request: PreparationRequest = serde_json::from_value(begin.body.clone()).unwrap();
    let begin_response: PreparationSubmitResponse =
        serde_json::from_value(begin.response.clone()).unwrap();
    begin_response.validate(&begin_request).expect(
        "the real ACK's one-day polling hint and two-day ETA must satisfy the existing SDK bounds",
    );
    assert_eq!(begin.response["submission"]["handle"], fixture::HANDLE);
    assert_eq!(
        begin.response["submission"]["status"]["poll_after_seconds"],
        fixture::POLL_AFTER_SECONDS
    );
    assert_eq!(
        begin.response["submission"]["status"]["eta_seconds"],
        fixture::ETA_SECONDS
    );
    let preparing = wait_phase(address, token, &preparation_path, "preparing").await;
    assert_eq!(preparing["progress_basis_points"], 2500);
    assert_eq!(preparing["eta_seconds"], fixture::ETA_SECONDS);
    assert!(
        preparing["next_check_at"].as_i64().unwrap()
            >= preparing["created_at"].as_i64().unwrap() + i64::from(fixture::POLL_AFTER_SECONDS)
    );
    assert_private(&preparing);
    assert_eq!(provider.counts(), (1, 0, 0, 0));
    hosts[0].request_shutdown();
    assert!(
        hosts[0].join().await,
        "the first host must join after the accepted handle is durable"
    );
    drop(services);
    age_stopped_fixture(directory, operation).await;

    let reopened = host::open(directory, "preparation-offline").await;
    hosts.push(Host::start(reopened).await);
    let address = hosts[1].address;
    let poll = tokio::time::timeout(Duration::from_secs(8), provider.next_poll()).await
        .expect("after two days offline the host must resume the accepted operation's current status using its persisted handle");
    assert_headers(&poll.headers);
    assert_eq!(
        poll.body,
        json!({
            "operation_id": operation, "recording_id": fixture::RECORDING, "source_id": fixture::SOURCE, "handle": fixture::HANDLE
        })
    );
    let poll_request: PreparationStatusRequest = serde_json::from_value(poll.body.clone()).unwrap();
    let poll_response: PreparationStatusResponse = serde_json::from_value(poll.response).unwrap();
    poll_response.validate(&poll_request).unwrap();
    let ready = wait_phase(address, token, &preparation_path, "ready").await;
    assert_eq!(ready["preparation_id"], operation);
    assert_eq!(ready["recording_id"], recording);
    assert_eq!(ready["source_id"], source);
    assert_eq!(ready["progress_basis_points"], 10000);
    assert_private(&ready);
    let after = http::get(address, token, &sources_path).await;
    assert_eq!(after.as_array().unwrap().len(), 1);
    assert_eq!(after[0]["id"], before[0]["id"]);
    assert_eq!(after[0]["recording_id"], recording);
    assert_eq!(after[0]["provider"], before[0]["provider"]);
    assert_eq!(after[0]["ready"], true);
    let response = http::call(
        address,
        "GET",
        &format!("/api/v1/sources/{source}/audio"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["content-type"], "audio/wav");
    assert_eq!(response.body, audio);
    let response = http::call(
        address,
        "POST",
        &preparation_path,
        Some(request),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert_eq!(response.json(), receipt);
    assert_eq!(http::get(address, token, "/api/v1/plugins").await, settings);
    assert_eq!(
        http::call(address, "GET", &download_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let recordings = http::get(address, token, "/api/v1/recordings").await;
    assert_eq!(recordings["total"], 1);
    assert_eq!(recordings["items"][0]["id"], recording);
    assert_eq!(provider.counts(), (1, 1, 1, 0));
}

async fn age_stopped_fixture(directory: &Path, operation: &str) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(directory.join("hito.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
    let changed = sqlx::query(
        "UPDATE source_preparations SET created_at = created_at - 172800,
        next_check_at = next_check_at - 172800 WHERE id = ?",
    )
    .bind(operation)
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(
        changed, 1,
        "only the stopped owned fixture operation's clock must be shifted"
    );
    pool.close().await;
}

async fn wait_phase(address: SocketAddr, token: &str, path: &str, phase: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let status = http::get(address, token, path).await;
            if status["phase"] == phase {
                return status;
            }
            assert!(matches!(
                status["phase"].as_str(),
                Some("queued" | "submitting" | "preparing")
            ));
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the real host must publish the {phase} phase"))
}

fn assert_headers(headers: &axum::http::HeaderMap) {
    assert_eq!(
        headers["authorization"],
        format!("Bearer {}", fixture::TOKEN)
    );
    assert_eq!(headers["x-hito-plugin-protocol"], "1");
}

fn assert_private(status: &Value) {
    let encoded = status.to_string();
    for private in [
        fixture::TOKEN,
        fixture::RECORDING,
        fixture::SOURCE,
        fixture::HANDLE,
    ] {
        assert!(!encoded.contains(private));
    }
}

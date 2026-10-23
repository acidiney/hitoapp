#[path = "fixtures/source_preparation_uncertainty.rs"]
mod fixture;
#[path = "fixtures/download_host.rs"]
mod host;
#[path = "fixtures/artwork_http.rs"]
mod http;
#[allow(dead_code)]
#[path = "api/support.rs"]
mod support;

use axum::http::StatusCode;
use futures_util::FutureExt;
use host::Host;
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn an_unacknowledged_begin_is_never_repeated_or_polled_after_restart_and_reconnection() {
    let directory = tempfile::tempdir().unwrap();
    let audio = support::write_wav(&directory.path().join("owned-uncertain.wav"));
    let mut provider = fixture::Provider::start(audio).await;
    let mut hosts = Vec::new();
    let outcome = AssertUnwindSafe(exercise(directory.path(), &provider, &mut hosts))
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

async fn exercise(directory: &Path, provider: &fixture::Provider, hosts: &mut Vec<Host>) {
    let services = host::open(directory, "preparation-uncertainty").await;
    hosts.push(Host::start(services.clone()).await);
    let address = hosts[0].address;
    let response = http::call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({
            "username": "owner", "password": "owned uncertainty test password"
        })),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    assert_eq!(response.headers["content-type"], "application/json");
    let session = response.json();
    let token = session["token"].as_str().unwrap();
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins/services",
        Some(json!({
            "name": "Owned uncertain provider", "base_url": provider.url
        })),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let service = response.json();
    let original = connect(address, token, &service).await;
    let (recording, source, original_sources) = adopt(address, token, &original).await;
    assert!(
        original_sources[0]["can_prepare"].as_bool() == Some(true),
        "the original active connection must offer preparation"
    );
    let path = format!("/api/v1/recordings/{recording}/sources/{source}/preparation");
    let request = json!({"request_id": "ab0d3897-53b3-4bbd-bc0e-585f73936fe2"});
    let response = http::call(address, "POST", &path, Some(request.clone()), Some(token)).await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let receipt = response.json();
    let operation = receipt["preparation_id"].as_str().unwrap();
    assert!(!Uuid::parse_str(operation).unwrap().is_nil());
    assert_eq!(receipt["request_id"], request["request_id"]);
    assert_eq!(receipt["recording_id"], recording);
    assert_eq!(receipt["source_id"], source);
    let begin = tokio::time::timeout(Duration::from_secs(3), provider.next_exchange())
        .await
        .expect("the real worker must dispatch one begin to the owned provider");
    assert_eq!(begin.path, "/v1/sources/prepare");
    assert_eq!(
        begin.headers["authorization"],
        format!("Bearer {}", fixture::TOKEN)
    );
    assert_eq!(begin.headers["x-hito-plugin-protocol"], "1");
    assert_eq!(
        begin.body,
        json!({
            "operation_id": operation, "recording_id": fixture::RECORDING, "source_id": fixture::SOURCE
        })
    );
    assert_eq!(
        provider.accepted_handle(operation).await,
        Some(fixture::HANDLE),
        "the upstream side effect must exist before Hito receives the truncated acknowledgement"
    );
    let unknown = wait_unknown(address, token, &path).await;
    assert_identity(&unknown, operation, &recording, &source);
    assert_eq!(provider.counts(), (1, 0, 0, 0));
    hosts[0].request_shutdown();
    assert!(
        hosts[0].join().await,
        "the first host must join after persisting the lost acknowledgement"
    );
    drop(services);

    let reopened = host::open(directory, "preparation-uncertainty").await;
    hosts.push(Host::start(reopened).await);
    let address = hosts[1].address;
    let unknown = http::get(address, token, &path).await;
    assert_eq!(unknown["phase"], "submission_unknown");
    assert_identity(&unknown, operation, &recording, &source);
    assert_uncertain(&unknown);
    let response = http::call(address, "POST", &path, Some(request.clone()), Some(token)).await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert_eq!(response.json(), receipt);
    let fresh_request = json!({"request_id": "885364d6-6edb-42bd-b3fd-f312ca038b8c"});
    let response = http::call(
        address,
        "POST",
        &path,
        Some(fresh_request.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let coalesced = response.json();
    assert_identity(&coalesced, operation, &recording, &source);
    assert_eq!(coalesced["request_id"], fresh_request["request_id"]);
    let no_dispatch = tokio::time::timeout(Duration::from_secs(7), provider.next_exchange()).await;
    assert!(
        no_dispatch.is_err(),
        "an unacknowledged begin must not begin again or poll without a known handle across the new five-second recovery window"
    );
    let unknown = http::get(address, token, &path).await;
    assert_eq!(unknown["phase"], "submission_unknown");
    assert_uncertain(&unknown);
    assert_eq!(provider.counts(), (1, 0, 0, 0));
    assert_eq!(
        http::get(
            address,
            token,
            &format!("/api/v1/recordings/{recording}/sources")
        )
        .await,
        original_sources
    );

    let response = http::call(
        address,
        "DELETE",
        &format!("/api/v1/plugins/{}", original["id"].as_str().unwrap()),
        Some(json!({"revision": original["revision"]})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let unknown = http::get(address, token, &path).await;
    assert_eq!(unknown["phase"], "submission_unknown");
    assert_uncertain(&unknown);
    let replacement = connect(address, token, &service).await;
    assert_ne!(replacement["id"], original["id"]);
    assert_eq!(replacement["service_id"], service["id"]);
    let (replacement_recording, replacement_source, replacement_sources) =
        adopt(address, token, &replacement).await;
    assert!(
        replacement_sources[0]["can_prepare"].as_bool() == Some(true),
        "the replacement active connection must offer preparation"
    );
    assert_ne!(replacement_recording, recording);
    assert_ne!(replacement_source, source);
    let replacement_path = format!(
        "/api/v1/recordings/{replacement_recording}/sources/{replacement_source}/preparation"
    );
    let response = http::call(
        address,
        "POST",
        &replacement_path,
        Some(json!({
            "request_id": "ac0f67b0-508c-481d-9620-845b69c56920"
        })),
        Some(token),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::CONFLICT,
        "recreating a connection must not duplicate the upstream side effect whose acknowledgement was lost"
    );
    let rejected = response.json();
    assert!(
        rejected["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    assert_private(&rejected);
    assert_eq!(
        http::call(address, "GET", &replacement_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    for (saved_request, saved_receipt) in [(request, receipt.clone()), (fresh_request, coalesced)] {
        let response = http::call(address, "POST", &path, Some(saved_request), Some(token)).await;
        assert_eq!(response.status, StatusCode::ACCEPTED);
        assert_eq!(response.json(), saved_receipt);
    }
    let unknown = http::get(address, token, &path).await;
    assert_eq!(unknown["phase"], "submission_unknown");
    assert_identity(&unknown, operation, &recording, &source);
    assert_uncertain(&unknown);
    let mut revoked_sources = original_sources;
    revoked_sources[0]["can_prepare"] = json!(false);
    for (recording, source, sources, can_prepare) in [
        (&recording, &source, revoked_sources, false),
        (
            &replacement_recording,
            &replacement_source,
            replacement_sources,
            true,
        ),
    ] {
        let current_sources = http::get(
            address,
            token,
            &format!("/api/v1/recordings/{recording}/sources"),
        )
        .await;
        assert!(
            current_sources[0]["can_prepare"].as_bool() == Some(can_prepare),
            "preparation eligibility must distinguish revoked and active connections"
        );
        assert!(
            current_sources == sources,
            "every source snapshot field must be preserved except revoked preparation eligibility"
        );
        assert_eq!(sources[0]["ready"], false);
        assert_eq!(
            http::call(
                address,
                "GET",
                &format!("/api/v1/sources/{source}/audio"),
                None,
                Some(token)
            )
            .await
            .status,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            http::call(
                address,
                "GET",
                &format!("/api/v1/recordings/{recording}/download"),
                None,
                Some(token)
            )
            .await
            .status,
            StatusCode::NOT_FOUND
        );
    }
    hosts[1].request_shutdown();
    assert!(
        hosts[1].join().await,
        "the restarted host must join without unresolved work being redispatched"
    );
    assert_eq!(provider.counts(), (1, 0, 0, 0));
    assert_eq!(
        provider.accepted_handle(operation).await,
        Some(fixture::HANDLE)
    );
}

async fn connect(address: SocketAddr, token: &str, service: &Value) -> Value {
    let response = http::call(address, "POST", "/api/v1/plugins", Some(json!({
        "service_id": service["id"], "service_revision": service["revision"], "bearer_token": fixture::TOKEN
    })), Some(token)).await;
    assert_eq!(response.status, StatusCode::CREATED);
    response.json()
}

async fn adopt(address: SocketAddr, token: &str, connection: &Value) -> (String, String, Value) {
    let response = http::call(address, "POST", &format!("/api/v1/plugins/{}/recordings", connection["id"].as_str().unwrap()),
        Some(json!({"installation_revision": connection["revision"], "provider_recording_id": fixture::RECORDING})), Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    let recording = response.json();
    assert_eq!(recording["available"], false);
    let recording = recording["id"].as_str().unwrap().to_owned();
    let sources = http::get(
        address,
        token,
        &format!("/api/v1/recordings/{recording}/sources"),
    )
    .await;
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert_eq!(sources[0]["ready"], false);
    let source = sources[0]["id"].as_str().unwrap().to_owned();
    (recording, source, sources)
}

async fn wait_unknown(address: SocketAddr, token: &str, path: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let status = http::get(address, token, path).await;
            if status["phase"] == "submission_unknown" {
                assert_uncertain(&status);
                return status;
            }
            assert!(matches!(
                status["phase"].as_str(),
                Some("queued" | "submitting")
            ));
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the truncated begin acknowledgement must become durable submission uncertainty")
}

fn assert_identity(value: &Value, operation: &str, recording: &str, source: &str) {
    assert_eq!(value["preparation_id"], operation);
    assert_eq!(value["recording_id"], recording);
    assert_eq!(value["source_id"], source);
    assert_private(value);
}

fn assert_uncertain(status: &Value) {
    assert!(status["progress_basis_points"].is_null());
    assert!(status["eta_seconds"].is_null());
    assert!(status["next_check_at"].is_null());
    assert_private(status);
}

fn assert_private(value: &Value) {
    let encoded = value.to_string();
    for private in [
        fixture::TOKEN,
        fixture::RECORDING,
        fixture::SOURCE,
        fixture::HANDLE,
    ] {
        assert!(!encoded.contains(private));
    }
}

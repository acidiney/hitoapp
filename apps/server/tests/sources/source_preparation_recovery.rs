#[path = "../fixtures/source_preparation_recovery.rs"]
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
use hito_core::SourcePreparationPhase;
use host::Host;
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn an_accepted_handle_keeps_polling_after_an_unknown_status_and_host_restart() {
    let directory = tempfile::tempdir().unwrap();
    let audio = support::write_wav(&directory.path().join("owned-recovery.wav"));
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
        "every real host and owned provider must join after the test"
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
    let services = host::open(directory, "preparation-recovery").await;
    hosts.push(Host::start(services.clone()).await);
    let address = hosts[0].address;
    let response = http::call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({"username": "owner", "password": "owned recovery test password"})),
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
        Some(json!({"name": "Owned recovery provider", "base_url": provider.url})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let service = response.json();
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins",
        Some(json!({
            "service_id": service["id"], "service_revision": service["revision"],
            "bearer_token": fixture::TOKEN
        })),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let installation = response.json();
    let response = http::call(
        address,
        "POST",
        &format!(
            "/api/v1/plugins/{}/recordings",
            installation["id"].as_str().unwrap()
        ),
        Some(json!({
            "installation_revision": installation["revision"],
            "provider_recording_id": fixture::RECORDING
        })),
        Some(token),
    )
    .await;
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
    let request = json!({"request_id": Uuid::new_v4()});
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
    assert_eq!(receipt["request_id"], request["request_id"]);
    assert_eq!(receipt["recording_id"], recording);
    assert_eq!(receipt["source_id"], source);

    let begin = tokio::time::timeout(Duration::from_secs(3), provider.next_begin())
        .await
        .expect("the real worker must submit the queued operation once");
    assert_exchange(&begin, operation, false);
    let preparing = wait_phase(address, token, &preparation_path, "preparing").await;
    assert_eq!(preparing["progress_basis_points"], 2500);
    assert_eq!(preparing["eta_seconds"], 15);
    assert_private_status(&preparing);
    let first_poll = tokio::time::timeout(Duration::from_secs(8), provider.next_poll())
        .await
        .expect("the worker must honor the five-second provider hint and poll the accepted handle");
    assert_exchange(&first_poll, operation, true);
    let unknown = wait_phase(address, token, &preparation_path, "submission_unknown").await;
    assert_eq!(unknown["preparation_id"], operation);
    assert!(unknown["progress_basis_points"].is_null());
    assert!(unknown["eta_seconds"].is_null());
    assert_private_status(&unknown);
    assert_eq!(http::get(address, token, &sources_path).await, before);
    assert_eq!(provider.counts(), (1, 1, 0, 0));
    assert_eq!(
        http::call(address, "GET", &download_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    hosts[0].request_shutdown();
    assert!(
        hosts[0].join().await,
        "the first host must join after persisting uncertainty"
    );
    drop(services);
    let reopened = host::open(directory, "preparation-recovery").await;
    let actor = reopened.identity.authenticate(token).await.unwrap();
    assert_eq!(actor.user().id, session["user"]["id"].as_str().unwrap());
    let retained = reopened
        .catalog
        .source_preparation(&actor, recording, source)
        .await
        .unwrap();
    assert_eq!(retained.preparation_id.to_string(), operation);
    assert!(matches!(
        retained.phase,
        SourcePreparationPhase::SubmissionUnknown
    ));
    assert!(retained.progress_basis_points.is_none());
    assert!(retained.eta_seconds.is_none());
    hosts.push(Host::start(reopened).await);
    let address = hosts[1].address;
    let next_poll = tokio::time::timeout(Duration::from_secs(8), provider.next_poll())
        .await
        .expect("the restarted host must keep polling the accepted handle after an unknown status without another begin");
    assert_exchange(&next_poll, operation, true);
    let ready = wait_phase(address, token, &preparation_path, "ready").await;
    assert_eq!(ready["preparation_id"], operation);
    assert_eq!(ready["recording_id"], recording);
    assert_eq!(ready["source_id"], source);
    assert_eq!(ready["progress_basis_points"], 10000);
    assert_private_status(&ready);
    let after = http::get(address, token, &sources_path).await;
    assert_eq!(after.as_array().unwrap().len(), 1);
    assert_eq!(after[0]["id"], source);
    assert_eq!(after[0]["recording_id"], recording);
    assert_eq!(after[0]["provider"], before[0]["provider"]);
    assert_eq!(after[0]["ready"], true);
    let catalog = http::get(address, token, "/api/v1/recordings").await;
    assert_eq!(catalog["total"], 1);
    assert_eq!(catalog["items"][0]["id"], recording);
    assert_eq!(catalog["items"][0]["available"], true);
    assert_eq!(http::get(address, token, "/api/v1/plugins").await, settings);
    assert_eq!(
        http::call(address, "GET", &download_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
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
    assert_eq!(provider.counts(), (1, 2, 1, 0));
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
                Some("queued" | "submitting" | "preparing" | "submission_unknown")
            ));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the real worker must publish the {phase} phase"))
}

fn assert_exchange(exchange: &fixture::Exchange, operation: &str, poll: bool) {
    assert_eq!(
        exchange.headers["authorization"],
        format!("Bearer {}", fixture::TOKEN)
    );
    assert_eq!(exchange.headers["x-hito-plugin-protocol"], "1");
    assert_eq!(exchange.body["operation_id"], operation);
    assert_eq!(exchange.body["recording_id"], fixture::RECORDING);
    assert_eq!(exchange.body["source_id"], fixture::SOURCE);
    let mut keys = exchange
        .body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    if poll {
        assert_eq!(exchange.body["handle"], fixture::HANDLE);
        assert_eq!(
            keys,
            ["handle", "operation_id", "recording_id", "source_id"]
        );
    } else {
        assert_eq!(keys, ["operation_id", "recording_id", "source_id"]);
    }
}

fn assert_private_status(status: &Value) {
    let status = status.to_string();
    for private in [
        fixture::TOKEN,
        fixture::RECORDING,
        fixture::SOURCE,
        fixture::HANDLE,
    ] {
        assert!(!status.contains(private));
    }
}

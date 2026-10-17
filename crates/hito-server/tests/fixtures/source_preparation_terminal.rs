use super::{
    RuntimeHost, SERVICE_TOKEN, assert_exchange, assert_private_status, capable, http, support,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::{net::SocketAddr, path::Path, time::Duration};
use uuid::Uuid;

pub(super) async fn run(
    directory: &Path,
    provider: &capable::CapableProvider,
    hosts: &mut Vec<RuntimeHost>,
    terminal_ack_after_removal: bool,
) {
    let services = support::services(directory, &directory.join("music")).await;
    hosts.push(RuntimeHost::start(services.clone()).await);
    let address = hosts[0].address;
    let response = http::call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({
            "username": "owner", "password": "owned terminal preparation password"
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
            "name": "Owned terminal provider", "base_url": provider.url
        })),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let service = response.json();
    let original = connect(address, token, &service).await;
    let (recording, sources) = adopt(address, token, &original).await;
    let source = source_id(&sources, false);
    let control = source_id(&sources, true);
    let actor = services.identity.authenticate(token).await.unwrap();
    let original_grant = services
        .catalog
        .playback_source(&actor, &recording, control)
        .await
        .unwrap();
    original_grant.check().await.unwrap();
    let path = format!("/api/v1/recordings/{recording}/sources/{source}/preparation");
    let request = json!({"request_id": "e5c1069b-7cd5-4ae3-88f3-935f9270891d"});
    let response = http::call(address, "POST", &path, Some(request.clone()), Some(token)).await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let receipt = response.json();
    assert_eq!(receipt["recording_id"], recording);
    assert_eq!(receipt["source_id"], source);
    let operation = receipt["preparation_id"].as_str().unwrap();
    assert!(!Uuid::parse_str(operation).unwrap().is_nil());
    let begin = tokio::time::timeout(Duration::from_secs(3), provider.next_begin())
        .await
        .expect("the first operation must reach the real provider transport");
    assert_exchange(&begin, operation, false);
    let mut pending_begin = Some(begin);
    if !terminal_ack_after_removal {
        pending_begin.take().unwrap().accept_failed();
        let failed = wait_phase(address, token, &path, "failed").await;
        assert_eq!(failed["reason"], "source_unavailable");
        assert_eq!(failed["preparation_id"], operation);
        assert_private_status(&failed);
    }
    let response = http::call(
        address,
        "DELETE",
        &format!("/api/v1/plugins/{}", original["id"].as_str().unwrap()),
        Some(json!({"revision": original["revision"]})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert!(original_grant.check().await.is_err());
    if terminal_ack_after_removal {
        let uncertain = http::get(address, token, &path).await;
        assert_eq!(uncertain["phase"], "submission_unknown");
        assert_private_status(&uncertain);
        pending_begin.take().unwrap().accept_failed();
    }
    let cancelled = wait_phase(address, token, &path, "cancelled").await;
    assert_eq!(cancelled["preparation_id"], operation);
    assert_eq!(cancelled["recording_id"], recording);
    assert_eq!(cancelled["source_id"], source);
    assert_private_status(&cancelled);
    assert_eq!(provider.counts(), (1, 0, 0));

    let replacement = connect(address, token, &service).await;
    assert_ne!(replacement["id"], original["id"]);
    assert_eq!(replacement["service_id"], original["service_id"]);
    let (replacement_recording, replacement_sources) = adopt(address, token, &replacement).await;
    assert_ne!(replacement_recording, recording);
    let replacement_source = source_id(&replacement_sources, false);
    assert_ne!(replacement_source, source);
    let replacement_control = source_id(&replacement_sources, true);
    let replacement_grant = services
        .catalog
        .playback_source(&actor, &replacement_recording, replacement_control)
        .await
        .unwrap();
    replacement_grant.check().await.unwrap();
    let settings = http::get(address, token, "/api/v1/plugins").await;
    let replacement_path = format!(
        "/api/v1/recordings/{replacement_recording}/sources/{replacement_source}/preparation"
    );
    let response = http::call(
        address,
        "POST",
        &replacement_path,
        Some(json!({
            "request_id": "40bcbec8-c8ca-4de0-8ba4-506f48aaf12f"
        })),
        Some(token),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::ACCEPTED,
        "a proven terminal remote outcome must permit a new operation after connection removal; late_ack={terminal_ack_after_removal}"
    );
    let replacement_receipt = response.json();
    assert_eq!(replacement_receipt["recording_id"], replacement_recording);
    assert_eq!(replacement_receipt["source_id"], replacement_source);
    let replacement_operation = replacement_receipt["preparation_id"].as_str().unwrap();
    assert!(!Uuid::parse_str(replacement_operation).unwrap().is_nil());
    assert_ne!(replacement_operation, operation);
    let begin = tokio::time::timeout(Duration::from_secs(3), provider.next_begin()).await
        .expect("the replacement must begin its own operation after the prior remote operation terminated");
    assert_exchange(&begin, replacement_operation, false);
    let submitting = http::get(address, token, &replacement_path).await;
    assert_eq!(submitting["phase"], "submitting");
    assert_private_status(&submitting);
    let response = http::call(address, "POST", &path, Some(request.clone()), Some(token)).await;
    assert_eq!(
        response.status,
        StatusCode::ACCEPTED,
        "an old immutable receipt must replay while the replacement is submitting without an origin uniqueness violation"
    );
    assert_eq!(response.json(), receipt);
    let old = http::get(address, token, &path).await;
    assert_eq!(old["phase"], "cancelled");
    assert_eq!(old["preparation_id"], operation);
    assert_private_status(&old);
    assert_eq!(provider.counts(), (2, 0, 0));
    begin.accept_failed();
    let failed = wait_phase(address, token, &replacement_path, "failed").await;
    assert_eq!(failed["reason"], "source_unavailable");
    assert_eq!(failed["preparation_id"], replacement_operation);
    assert_private_status(&failed);
    let response = http::call(address, "POST", &path, Some(request), Some(token)).await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert_eq!(response.json(), receipt);
    let old_sources = http::get(
        address,
        token,
        &format!("/api/v1/recordings/{recording}/sources"),
    )
    .await;
    assert_eq!(
        old_sources.as_array().unwrap().len(),
        sources.as_array().unwrap().len()
    );
    for previous in sources.as_array().unwrap() {
        let current = old_sources
            .as_array()
            .unwrap()
            .iter()
            .find(|source| source["id"] == previous["id"])
            .unwrap();
        assert_eq!(current["recording_id"], recording);
        assert_eq!(current["provider"], previous["provider"]);
        assert_eq!(current["ready"], false);
    }
    assert_eq!(
        http::get(
            address,
            token,
            &format!("/api/v1/recordings/{replacement_recording}/sources")
        )
        .await,
        replacement_sources
    );
    assert_eq!(http::get(address, token, "/api/v1/plugins").await, settings);
    replacement_grant.check().await.unwrap();
    for recording in [&recording, &replacement_recording] {
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
    assert_eq!(provider.counts(), (2, 0, 0));
}

async fn connect(address: SocketAddr, token: &str, service: &Value) -> Value {
    let response = http::call(address, "POST", "/api/v1/plugins", Some(json!({
        "service_id": service["id"], "service_revision": service["revision"], "bearer_token": SERVICE_TOKEN
    })), Some(token)).await;
    assert_eq!(response.status, StatusCode::CREATED);
    response.json()
}

async fn adopt(address: SocketAddr, token: &str, connection: &Value) -> (String, Value) {
    let response = http::call(address, "POST", &format!(
        "/api/v1/plugins/{}/recordings", connection["id"].as_str().unwrap()
    ), Some(json!({
        "installation_revision": connection["revision"], "provider_recording_id": capable::RECORDING
    })), Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    let recording = response.json()["id"].as_str().unwrap().to_owned();
    let sources = http::get(
        address,
        token,
        &format!("/api/v1/recordings/{recording}/sources"),
    )
    .await;
    assert_eq!(sources.as_array().unwrap().len(), 2);
    (recording, sources)
}

fn source_id(sources: &Value, ready: bool) -> &str {
    sources
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["ready"] == ready)
        .unwrap()["id"]
        .as_str()
        .unwrap()
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
                Some("queued" | "submitting" | "submission_unknown")
            ));
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the real host must retain the exact acknowledged terminal outcome")
}

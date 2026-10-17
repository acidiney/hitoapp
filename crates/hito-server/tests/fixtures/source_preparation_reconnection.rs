use super::{
    RuntimeHost, SERVICE_TOKEN, assert_exchange, assert_private_status, capable, http, support,
};
use axum::http::StatusCode;
use serde_json::json;
use std::{path::Path, time::Duration};
use uuid::Uuid;

pub(super) async fn run(
    directory: &Path,
    provider: &capable::CapableProvider,
    hosts: &mut Vec<RuntimeHost>,
) {
    let services = support::services(directory, &directory.join("music")).await;
    hosts.push(RuntimeHost::start(services.clone()).await);
    let address = hosts[0].address;
    let response = http::call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({
            "username": "owner", "password": "owned unresolved preparation password"
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
            "name": "Owned unresolved provider", "base_url": provider.url
        })),
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
            "bearer_token": SERVICE_TOKEN
        })),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let original_connection = response.json();
    let response = http::call(address, "POST", &format!(
        "/api/v1/plugins/{}/recordings", original_connection["id"].as_str().unwrap()
    ), Some(json!({
        "installation_revision": original_connection["revision"], "provider_recording_id": capable::RECORDING
    })), Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    let original_recording = response.json();
    let recording = original_recording["id"].as_str().unwrap();
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    let original_sources = http::get(address, token, &sources_path).await;
    assert_eq!(original_sources.as_array().unwrap().len(), 2);
    let source = original_sources
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["ready"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let original_control = original_sources
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["ready"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let actor = services.identity.authenticate(token).await.unwrap();
    let control = services
        .catalog
        .playback_source(&actor, recording, original_control)
        .await
        .unwrap();
    control.check().await.unwrap();
    let preparation_path = format!("{sources_path}/{source}/preparation");
    let request = json!({"request_id": "c4fa174e-10e0-439a-8fcb-aa1a40637d51"});
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
    assert_eq!(receipt["recording_id"], recording);
    assert_eq!(receipt["source_id"], source);
    let operation = receipt["preparation_id"].as_str().unwrap();
    assert!(!Uuid::parse_str(operation).unwrap().is_nil());
    let begin = tokio::time::timeout(Duration::from_secs(3), provider.next_begin())
        .await
        .expect("the original connection must submit the durable preparation once");
    assert_exchange(&begin, operation, false);
    begin.accept();
    let poll = tokio::time::timeout(Duration::from_secs(8), provider.next_poll())
        .await
        .expect("the acknowledged handle must be persisted and polled before revocation");
    assert_exchange(&poll, operation, true);
    let preparing = http::get(address, token, &preparation_path).await;
    assert_eq!(preparing["phase"], "preparing");
    assert_eq!(preparing["progress_basis_points"], 2500);
    assert_private_status(&preparing);
    assert_eq!(provider.counts(), (1, 1, 0));

    let response = http::call(
        address,
        "DELETE",
        &format!(
            "/api/v1/plugins/{}",
            original_connection["id"].as_str().unwrap()
        ),
        Some(json!({"revision": original_connection["revision"]})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert!(
        control.check().await.is_err(),
        "removing the connection must revoke its existing playback authority"
    );
    let cancelled = http::get(address, token, &preparation_path).await;
    assert_eq!(cancelled["phase"], "cancelled");
    assert_eq!(cancelled["preparation_id"], operation);
    assert_eq!(cancelled["recording_id"], recording);
    assert_eq!(cancelled["source_id"], source);
    assert_private_status(&cancelled);
    let response = http::call(
        address,
        "POST",
        &preparation_path,
        Some(request.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert_eq!(response.json(), receipt);

    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins",
        Some(json!({
            "service_id": service["id"], "service_revision": service["revision"],
            "bearer_token": SERVICE_TOKEN
        })),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let replacement = response.json();
    assert_ne!(replacement["id"], original_connection["id"]);
    assert_eq!(replacement["service_id"], service["id"]);
    let response = http::call(address, "POST", &format!(
        "/api/v1/plugins/{}/recordings", replacement["id"].as_str().unwrap()
    ), Some(json!({
        "installation_revision": replacement["revision"], "provider_recording_id": capable::RECORDING
    })), Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    let replacement_recording = response.json();
    let replacement_recording = replacement_recording["id"].as_str().unwrap();
    assert_ne!(replacement_recording, recording);
    let replacement_sources_path = format!("/api/v1/recordings/{replacement_recording}/sources");
    let replacement_sources = http::get(address, token, &replacement_sources_path).await;
    assert_eq!(replacement_sources.as_array().unwrap().len(), 2);
    let replacement_source = replacement_sources
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["ready"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_ne!(replacement_source, source);
    let replacement_control = replacement_sources
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["ready"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_ne!(replacement_control, original_control);
    let replacement_grant = services
        .catalog
        .playback_source(&actor, replacement_recording, replacement_control)
        .await
        .unwrap();
    replacement_grant.check().await.unwrap();
    let settings = http::get(address, token, "/api/v1/plugins").await;
    let replacement_preparation_path =
        format!("{replacement_sources_path}/{replacement_source}/preparation");
    let response = http::call(
        address,
        "POST",
        &replacement_preparation_path,
        Some(json!({
            "request_id": "26d12c67-bc58-4dc8-8795-dbd457b7825f"
        })),
        Some(token),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::CONFLICT,
        "a recreated connection must block a second begin for the unresolved exact provider origin"
    );
    let rejection = response.json();
    assert!(
        rejection["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    assert_private_status(&rejection);
    assert_eq!(
        http::call(
            address,
            "GET",
            &replacement_preparation_path,
            None,
            Some(token)
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "a rejected replacement must not leave a new preparation intent"
    );
    assert_eq!(provider.counts(), (1, 1, 0));

    poll.ready_if_waiting();
    hosts[0].finish().await;
    hosts.push(RuntimeHost::start(services).await);
    let address = hosts[1].address;
    let cancelled = http::get(address, token, &preparation_path).await;
    assert_eq!(cancelled["phase"], "cancelled");
    assert_eq!(cancelled["preparation_id"], operation);
    assert_private_status(&cancelled);
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
    let old_sources = http::get(address, token, &sources_path).await;
    assert_eq!(old_sources.as_array().unwrap().len(), 2);
    for previous in original_sources.as_array().unwrap() {
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
        http::get(address, token, &replacement_sources_path).await,
        replacement_sources
    );
    assert_eq!(http::get(address, token, "/api/v1/plugins").await, settings);
    replacement_grant.check().await.expect(
        "a rejected preparation must preserve the replacement control's playback authority",
    );
    let recordings = http::get(address, token, "/api/v1/recordings").await;
    assert_eq!(recordings["total"], 2);
    for expected in [recording, replacement_recording] {
        assert!(
            recordings["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|recording| recording["id"] == expected)
        );
    }
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
        StatusCode::NOT_FOUND,
        "a late ready response cannot restore removed connection authority"
    );
    for recording in [recording, replacement_recording] {
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
    assert_eq!(provider.counts(), (1, 1, 0));
}

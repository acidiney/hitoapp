use super::host::Host;
use crate::contract::fixture;
use hito_core::Services;
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use std::future::pending;

pub(super) async fn adopt(
    client: &Client,
    host: &fixture::Server,
    plugin: &str,
) -> (String, String, Value) {
    let session = call(
        client,
        host,
        Method::POST,
        "api/v1/setup",
        json!({"username": "owner", "password": "private-owned-host-password"}),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap().to_owned();
    let service = call(
        client,
        host,
        Method::POST,
        "api/v1/plugins/services",
        json!({"name": "Owned Jamendo host service", "base_url": plugin}),
        Some(&token),
        StatusCode::CREATED,
    )
    .await;
    let draft = json!({
        "service_id": service["id"], "service_revision": service["revision"],
        "bearer_token": null
    });
    let preview = call(
        client,
        host,
        Method::POST,
        "api/v1/plugins/preview",
        draft.clone(),
        Some(&token),
        StatusCode::OK,
    )
    .await;
    assert!(
        preview["manifest"]["id"] == "org.hito.jamendo",
        "unexpected provider manifest identity"
    );
    assert!(
        preview["manifest"]["content_policy"]["source_persistence"] == "playback_only",
        "unexpected provider persistence policy"
    );
    let mut accepted = draft;
    accepted["accept_manifest_digest"] = preview["digest"].clone();
    let installation = call(
        client,
        host,
        Method::POST,
        "api/v1/plugins",
        accepted,
        Some(&token),
        StatusCode::CREATED,
    )
    .await;
    let id = installation["id"].as_str().unwrap();
    let page = call(
        client,
        host,
        Method::POST,
        &format!("api/v1/plugins/{id}/search"),
        json!({"installation_revision": installation["revision"], "query": fixture::QUERY,
               "cursor": null, "limit": 10}),
        Some(&token),
        StatusCode::OK,
    )
    .await;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_private_transport_absent(&page);
    let recording = call(
        client,
        host,
        Method::POST,
        &format!("api/v1/plugins/{id}/recordings"),
        json!({"installation_revision": installation["revision"],
               "provider_recording_id": page["items"][0]["id"]}),
        Some(&token),
        StatusCode::OK,
    )
    .await;
    let recording = recording["id"].as_str().unwrap().to_owned();
    let metadata = call(
        client,
        host,
        Method::GET,
        &format!("api/v1/recordings/{recording}/metadata"),
        Value::Null,
        Some(&token),
        StatusCode::OK,
    )
    .await;
    let observation = &metadata["selected_observation"];
    assert!(
        observation["provider"] == "org.hito.jamendo",
        "unexpected retained provider identity"
    );
    assert!(observation["provider_recording_id"] == page["items"][0]["id"]);
    assert_attribution(observation);
    assert!(metadata["observations"]["items"][0] == *observation);
    assert_eq!(
        metadata["observations"]["items"].as_array().unwrap().len(),
        1
    );
    assert_private_transport_absent(&metadata);
    call(
        client,
        host,
        Method::GET,
        &format!("api/v1/recordings/{recording}/download"),
        Value::Null,
        Some(&token),
        StatusCode::NOT_FOUND,
    )
    .await;
    (token, recording, metadata)
}

pub(super) async fn assert_metadata_only(provider: &fixture::Provider, services: &Services) {
    assert_eq!(
        provider.media_requests(),
        0,
        "metadata must retrieve zero audio"
    );
    assert_eq!(provider.download_requests(), 0);
    assert_eq!(provider.artwork_requests(), 0);
    assert!(
        !services
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap_or_else(|_| panic!("owned download worker failed"))
    );
}

pub(super) async fn call(
    client: &Client,
    host: &fixture::Server,
    method: Method,
    path: &str,
    body: Value,
    token: Option<&str>,
    expected: StatusCode,
) -> Value {
    let mut request = client
        .request(method, host.base.join(path).unwrap())
        .header("x-hito-request", "1")
        .header("cookie", "owned-marker=private-host-cookie");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if !body.is_null() {
        request = request.json(&body);
    }
    received_response(request.send().await, expected).await
}

pub(super) async fn received_response(
    response: Result<reqwest::Response, reqwest::Error>,
    expected: StatusCode,
) -> Value {
    let response = response.unwrap_or_else(|_| panic!("owned host HTTP request failed"));
    assert_eq!(response.status(), expected);
    json_body(response).await
}

pub(super) async fn json_body(response: reqwest::Response) -> Value {
    let bytes = response
        .bytes()
        .await
        .unwrap_or_else(|_| panic!("owned host HTTP body failed"));
    assert!(
        bytes.len() <= 262_144,
        "owned host response exceeded its bound"
    );
    serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("owned host response was not JSON"))
}

pub(super) fn assert_private_transport_absent(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                if name == "content_url" {
                    assert!(
                        value.as_str() == Some(fixture::CONTENT_URL),
                        "host response exposed private transport data"
                    );
                } else {
                    assert_private_transport_absent(value);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(assert_private_transport_absent),
        Value::String(text) => {
            for private in [
                fixture::CLIENT_ID,
                "private-",
                "http://",
                "https://",
                "127.0.0.1",
                "share.invalid",
                "short.invalid",
            ] {
                assert!(
                    !text.contains(private),
                    "host response exposed private transport data"
                );
            }
        }
        _ => {}
    }
}

pub(super) async fn audio(
    owned: &Host,
    token: &str,
    source: &str,
    range: Option<&str>,
) -> reqwest::Response {
    let mut request = owned
        .client
        .get(
            owned
                .server()
                .base
                .join(&format!("api/v1/sources/{source}/audio"))
                .unwrap(),
        )
        .bearer_auth(token)
        .header("cookie", "owned-marker=private-playback-cookie");
    if let Some(range) = range {
        request = request.header("range", range);
    }
    request
        .send()
        .await
        .unwrap_or_else(|_| panic!("owned host audio request failed"))
}

pub(super) fn assert_attribution(observation: &Value) {
    assert!(
        observation["attribution"]
            == json!({
                "creators": [fixture::ARTIST], "provider_label": "Jamendo", "content_url": fixture::CONTENT_URL
            }),
        "retained Jamendo attribution did not match"
    );
}

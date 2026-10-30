#[path = "../fixtures/plugin_install_preview_collection.rs"]
mod collection;
#[path = "../fixtures/plugin_install_preview.rs"]
mod fixture;
use collection::{account_state, seed_collection};
#[path = "../fixtures/download_host.rs"]
mod host;
#[path = "../fixtures/artwork_http.rs"]
mod http;
#[allow(dead_code)]
#[path = "../api/support.rs"]
mod support;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::FutureExt;
use hito_plugin_sdk::Manifest;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn installation_preview_returns_review_without_mutating_account_state() {
    workflow(Some("explicit_download")).await;
}

#[tokio::test]
async fn playback_only_installation_requires_the_exact_reviewed_manifest() {
    workflow(Some("playback_only")).await;
}

#[tokio::test]
async fn undeclared_installation_preserves_legacy_omission_of_review() {
    workflow(None).await;
}

async fn workflow(persistence: Option<&str>) {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let original_path = music.join("owned-original.wav");
    let original = support::write_wav(&original_path);
    let mut manifest = json!({
        "id": "org.hito.http-install-preview",
        "name": "Owned installation preview provider",
        "protocol_version": 1,
        "capabilities": ["search", "sources"]
    });
    if let Some(persistence) = persistence {
        manifest["content_policy"] = json!({"source_persistence": persistence});
    }
    let mut provider = fixture::Provider::start(manifest.clone()).await;
    let services = host::open(directory.path(), "http-install-preview").await;
    let app = hito_server::router(services.clone());
    let mut host = host::Host::start(services).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise_preview(host.address, &app, &provider, &manifest)).catch_unwind(),
    )
    .await;
    host.request_shutdown();
    let host_joined = host.join().await;
    let provider_joined = provider.finish().await;
    assert!(
        host_joined && provider_joined,
        "owned HTTP servers must stop and join"
    );
    assert!(
        std::fs::read(original_path).unwrap() == original,
        "preview must preserve the owned original audio"
    );
    assert_eq!(std::fs::read_dir(music).unwrap().count(), 1);
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public installation preview must finish within its deadline"),
    }
}

async fn exercise_preview(
    address: SocketAddr,
    app: &Router,
    provider: &fixture::Provider,
    manifest: &Value,
) {
    let response = http::call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({"username": "owner", "password": "owned installation preview password"})),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let session = response.json();
    let token = session["token"].as_str().unwrap();
    seed_collection(address, token).await;
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins/services",
        Some(json!({"name": "Owned preview service", "base_url": provider.url})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let service = response.json();
    let draft = json!({
        "service_id": service["id"],
        "service_revision": service["revision"],
        "bearer_token": fixture::TOKEN
    });
    assert_transport_guards(app, token, &draft).await;
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins/preview",
        Some(draft.clone()),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    let mut unknown = draft.clone();
    unknown["base_url"] = json!("http://unoffered.invalid/");
    let mut stale = draft.clone();
    stale["service_revision"] = json!(service["revision"].as_i64().unwrap() + 1);
    let mut missing = draft.clone();
    missing["service_id"] = json!(Uuid::new_v4());
    for (body, expected) in [
        (unknown, StatusCode::UNPROCESSABLE_ENTITY),
        (stale, StatusCode::CONFLICT),
        (missing, StatusCode::NOT_FOUND),
    ] {
        let response = http::call(
            address,
            "POST",
            "/api/v1/plugins/preview",
            Some(body),
            Some(token),
        )
        .await;
        assert_eq!(response.status, expected);
        assert_eq!(response.headers["cache-control"], "private, no-store");
        assert_private_error(&response.json(), provider);
    }
    provider.assert_manifest_requests(&[]).await;
    let before = account_state(address, token).await;
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins/preview",
        Some(draft.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["cache-control"], "private, no-store");
    let preview = response.json();
    let typed_manifest: Manifest = serde_json::from_value(preview["manifest"].clone()).unwrap();
    typed_manifest.validate().unwrap();
    assert!(serde_json::to_value(typed_manifest).unwrap() == *manifest);
    let digest = preview["digest"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(!preview.to_string().contains(fixture::TOKEN));
    assert!(preview.get("credential").is_none() && preview.get("credential_set").is_none());
    assert!(account_state(address, token).await == before);
    assert!(before["plugins"]["installations"] == json!([]));
    provider.assert_manifest_requests(&[true]).await;
    exercise_acceptance(
        address, token, provider, manifest, &draft, &preview, &before,
    )
    .await;
}

async fn exercise_acceptance(
    address: SocketAddr,
    token: &str,
    provider: &fixture::Provider,
    manifest: &Value,
    draft: &Value,
    preview: &Value,
    before: &Value,
) {
    let mut requests = vec![true];
    if manifest.get("content_policy").is_some() {
        let response = http::call(
            address,
            "POST",
            "/api/v1/plugins",
            Some(draft.clone()),
            Some(token),
        )
        .await;
        assert_eq!(response.status, StatusCode::CONFLICT);
        assert_private_error(&response.json(), provider);
        assert!(account_state(address, token).await == *before);
        requests.push(true);
        provider.assert_manifest_requests(&requests).await;
    }
    let mut acceptance = draft.clone();
    acceptance["accept_manifest_digest"] = preview["digest"].clone();
    let mut changed = manifest.clone();
    if changed
        .as_object_mut()
        .unwrap()
        .remove("content_policy")
        .is_none()
    {
        changed["name"] = json!("Changed installation preview provider");
    }
    provider.reply(changed).await;
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins",
        Some(acceptance.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_private_error(&response.json(), provider);
    assert!(account_state(address, token).await == *before);
    requests.push(true);
    provider.assert_manifest_requests(&requests).await;

    provider.reply(manifest.clone()).await;
    let mut without_bearer = acceptance.clone();
    without_bearer
        .as_object_mut()
        .unwrap()
        .remove("bearer_token");
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins",
        Some(without_bearer),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::BAD_GATEWAY);
    assert_private_error(&response.json(), provider);
    assert!(account_state(address, token).await == *before);
    requests.push(false);
    provider.assert_manifest_requests(&requests).await;

    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins",
        Some(acceptance),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let installation = response.json();
    assert!(installation["manifest"] == preview["manifest"]);
    assert!(installation["credential_set"] == true && installation["revision"] == 1);
    assert!(!installation.to_string().contains(fixture::TOKEN));
    requests.push(true);
    provider.assert_manifest_requests(&requests).await;
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins/preview",
        Some(draft.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    provider.assert_manifest_requests(&requests).await;
    remove_installation(address, token, &installation).await;
    assert!(account_state(address, token).await == *before);

    let mut legacy = manifest.clone();
    legacy.as_object_mut().unwrap().remove("content_policy");
    provider.reply(legacy).await;
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins",
        Some(draft.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let legacy = response.json();
    assert!(legacy["manifest"].get("content_policy").is_none());
    requests.push(true);
    provider.assert_manifest_requests(&requests).await;
    remove_installation(address, token, &legacy).await;
    assert!(account_state(address, token).await == *before);

    let response = http::call(
        address,
        "DELETE",
        &format!(
            "/api/v1/plugins/services/{}",
            draft["service_id"].as_str().unwrap()
        ),
        Some(json!({"revision": draft["service_revision"]})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins/preview",
        Some(draft.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    provider.assert_manifest_requests(&requests).await;
    let response = http::call(address, "POST", "/api/v1/logout", None, Some(token)).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins/preview",
        Some(draft.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    provider.assert_manifest_requests(&requests).await;
}

async fn remove_installation(address: SocketAddr, token: &str, installation: &Value) {
    let response = http::call(
        address,
        "DELETE",
        &format!("/api/v1/plugins/{}", installation["id"].as_str().unwrap()),
        Some(json!({"revision": installation["revision"]})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
}

fn assert_private_error(error: &Value, provider: &fixture::Provider) {
    assert!(
        error["error"]
            .as_str()
            .is_some_and(|message| message.len() <= 256)
    );
    let encoded = error.to_string();
    for private in [
        fixture::TOKEN,
        provider.url.as_str(),
        "Owned installation preview provider",
    ] {
        assert!(!encoded.contains(private));
    }
}

async fn assert_transport_guards(app: &Router, token: &str, draft: &Value) {
    let requests = [
        Request::builder()
            .header("authorization", format!("Bearer {token}"))
            .header("x-hito-account", Uuid::new_v4().to_string()),
        Request::builder().header("cookie", format!("hito_session={token}")),
        Request::builder()
            .header("authorization", format!("Bearer {token}"))
            .header("origin", "https://untrusted.invalid"),
    ];
    for request in requests {
        let response = app
            .clone()
            .oneshot(
                request
                    .method("POST")
                    .uri("/api/v1/plugins/preview")
                    .header("content-type", "application/json")
                    .body(Body::from(draft.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let error: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(error["error"].as_str().is_some());
        assert!(!error.to_string().contains(fixture::TOKEN));
    }
}

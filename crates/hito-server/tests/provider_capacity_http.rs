#[path = "fixtures/download_host.rs"]
mod host;
#[path = "fixtures/artwork_http.rs"]
mod http;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;

use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::json;
use std::{panic::AssertUnwindSafe, time::Duration};

#[tokio::test]
async fn typed_provider_capacity_preserves_the_existing_http_conflict_contract() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let services = host::open(directory.path(), "provider-capacity-http").await;
    let mut server = host::Host::start(services).await;
    let provider = provider::ProviderServer::start().await;
    let outcome = tokio::time::timeout(Duration::from_secs(12), AssertUnwindSafe(async {
        let setup = http::call(server.address, "POST", "/api/v1/setup",
            Some(json!({"username": "owner", "password": "private capacity HTTP password"})), None).await;
        assert!(setup.status == StatusCode::CREATED, "the owned account must be initialized");
        let session = setup.json();
        let token = session["token"].as_str().unwrap();
        let offered = http::call(server.address, "POST", "/api/v1/plugins/services",
            Some(json!({"name": "Owned capacity provider", "base_url": provider.url})), Some(token)).await;
        assert!(offered.status == StatusCode::CREATED, "the owned plugin service must be offered");
        let service = offered.json();
        let draft = json!({
            "service_id": service["id"], "service_revision": service["revision"],
            "bearer_token": null, "accept_manifest_digest": null
        });
        let gate = provider.hold("v1/manifest").await;
        let (accepted, ()) = tokio::join!(
            http::call(server.address, "POST", "/api/v1/plugins", Some(draft.clone()), Some(token)),
            async {
                gate.entered().await;
                let before = http::get(server.address, token, "/api/v1/plugins").await;
                let busy = http::call(server.address, "POST", "/api/v1/plugins",
                    Some(draft.clone()), Some(token)).await;
                assert!(busy.status == StatusCode::CONFLICT,
                    "typed provider capacity must retain HTTP409");
                assert!(busy.headers.get("content-type").is_some_and(|value| value == "application/json"),
                    "the capacity error must retain its JSON response type");
                assert!(busy.json() == json!({
                    "error": "Provider requests for this account are busy. Try again."
                }), "capacity rejection must retain the exact safe message and omit an error code");
                let mut stale = draft.clone();
                stale["service_revision"] = json!(service["revision"].as_i64().unwrap() + 1);
                let changed = http::call(server.address, "POST", "/api/v1/plugins",
                    Some(stale), Some(token)).await;
                assert!(changed.status == StatusCode::CONFLICT,
                    "authority revision conflicts must retain HTTP409 independently of capacity");
                assert!(changed.json() == json!({"error": "This plugin service changed. Reload Settings."}),
                    "a stale service revision must retain its distinct authority message and omit an error code");
                let after = http::get(server.address, token, "/api/v1/plugins").await;
                assert!(after == before,
                    "rejected capacity and authority requests must preserve exact account plugin state");
                assert!(provider.requests("v1/manifest").await.len() == 1
                    && provider.requests("v1/sources").await.is_empty()
                    && provider.requests("v1/audio").await.is_empty(),
                    "rejected requests must not dispatch extra provider work");
                gate.open();
            },
        );
        assert!(accepted.status == StatusCode::CREATED,
            "the held owner must still complete its accepted installation");
        let settings = http::get(server.address, token, "/api/v1/plugins").await;
        assert!(settings["installations"].as_array().is_some_and(|items| items.len() == 1)
            && provider.requests("v1/manifest").await.len() == 1,
            "only the original accepted owner may publish a plugin installation");
    }).catch_unwind()).await;
    server.request_shutdown();
    let joined = server.join().await;
    provider.shutdown().await;
    assert!(joined, "the owned HTTP host must stop and join");
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => {
            panic!("the owned provider capacity HTTP workflow must settle within its deadline")
        }
    }
}

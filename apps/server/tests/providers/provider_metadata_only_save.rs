#[path = "../fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "../fixtures/artwork_http.rs"]
mod http;
use hito_core::test_support::content_service as provider;

use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::json;
use std::{panic::AssertUnwindSafe, time::Duration};

#[tokio::test]
async fn saving_from_a_provider_without_sources_succeeds_with_no_provider_sources() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let services = host::open(directory.path(), "metadata-only-save-http").await;
    let mut server = host::Host::start(services).await;
    let provider = provider::ProviderServer::start().await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(12),
        AssertUnwindSafe(async {
            provider
                .reply(
                    "v1/manifest",
                    json!({
                        "id": "org.hito.metadata-only-fixture", "name": "Metadata only",
                        "protocol_version": 1, "capabilities": ["search", "metadata", "artwork"]
                    }),
                )
                .await;
            provider
                .reply(
                    "v1/recordings/detail",
                    json!({"id": "item/1", "title": "Saved tune", "artist": "Saved artist",
                        "duration_ms": 61000, "revision": "r1"}),
                )
                .await;
            let setup = http::call(
                server.address,
                "POST",
                "/api/v1/setup",
                Some(json!({"username": "owner", "password": "metadata only HTTP password"})),
                None,
            )
            .await;
            assert!(setup.status == StatusCode::CREATED);
            let session = setup.json();
            let token = session["token"].as_str().unwrap();
            let offered = http::call(
                server.address,
                "POST",
                "/api/v1/plugins/services",
                Some(json!({"name": "Metadata only", "base_url": provider.url})),
                Some(token),
            )
            .await;
            assert!(offered.status == StatusCode::CREATED);
            let service = offered.json();
            let installed = http::call(
                server.address,
                "POST",
                "/api/v1/plugins",
                Some(json!({
                    "service_id": service["id"], "service_revision": service["revision"],
                    "bearer_token": null, "accept_manifest_digest": null
                })),
                Some(token),
            )
            .await;
            assert!(installed.status == StatusCode::CREATED);
            let installation = installed.json();
            let saved = http::call(
                server.address,
                "POST",
                &format!(
                    "/api/v1/plugins/{}/recordings",
                    installation["id"].as_str().unwrap()
                ),
                Some(json!({
                    "installation_revision": installation["revision"],
                    "provider_recording_id": "item/1"
                })),
                Some(token),
            )
            .await;
            assert!(
                saved.status == StatusCode::OK,
                "a provider without sources must still save recordings"
            );
            let recording = saved.json();
            assert!(recording["title"] == "Saved tune" && recording["available"] == false);
            let sources = http::get(
                server.address,
                token,
                &format!(
                    "/api/v1/recordings/{}/sources",
                    recording["id"].as_str().unwrap()
                ),
            )
            .await;
            assert!(sources.as_array().is_some_and(Vec::is_empty));
            assert!(provider.requests("v1/sources").await.is_empty());
        })
        .catch_unwind(),
    )
    .await;
    server.request_shutdown();
    let joined = server.join().await;
    provider.shutdown().await;
    assert!(joined, "the owned HTTP host must stop and join");
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the metadata-only save workflow must settle within its deadline"),
    }
}

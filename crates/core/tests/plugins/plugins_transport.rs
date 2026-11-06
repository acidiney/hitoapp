use hito_core::test_support::plugin_service as fixture;
use hito_core::test_support::support;

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use hito_core::{Error, InstallPlugin, OfferPluginService};

#[tokio::test]
async fn redirects_errors_non_json_and_oversized_bodies_leave_no_installation() {
    let target = fixture::PluginServer::start().await;
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-transport-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let mut redirect = HeaderMap::new();
    redirect.insert(
        "location",
        HeaderValue::from_str(&format!("{}v1/manifest", target.url)).unwrap(),
    );
    for (status, headers, body, chunked) in [
        (StatusCode::FOUND, redirect, vec![], false),
        (
            StatusCode::UNAUTHORIZED,
            HeaderMap::new(),
            b"private upstream error".to_vec(),
            false,
        ),
        (
            StatusCode::OK,
            HeaderMap::new(),
            serde_json::to_vec(&fixture::manifest()).unwrap(),
            false,
        ),
        (StatusCode::OK, json_headers(), vec![b' '; 65537], false),
        (StatusCode::OK, json_headers(), vec![b' '; 65537], true),
    ] {
        server.response(status, headers, body, chunked).await;
        let before = server.hits();
        let error = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id.clone(),
                    service_revision: service.revision,
                    bearer_token: None,
                },
            )
            .await
            .err()
            .unwrap();
        assert!(matches!(error, Error::Upstream(_)));
        assert!(!format!("{error:?}").contains("private upstream error"));
        assert_eq!(
            server.hits(),
            before + 1,
            "failed manifest requests are not retried"
        );
        assert!(
            hito.plugins
                .settings(&actor)
                .await
                .unwrap()
                .installations
                .is_empty()
        );
    }
    assert_eq!(target.hits(), 0, "redirects must not be followed");
}

fn json_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers
}

#[tokio::test]
async fn unavailable_plugin_requests_expire_and_release_admission_for_retry() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-deadline-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let gate = server.hold().await;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(14),
        hito.plugins.install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id.clone(),
                service_revision: 1,
                bearer_token: None,
            },
        ),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(Error::Upstream(_))));
    assert!(
        hito.plugins
            .settings(&actor)
            .await
            .unwrap()
            .installations
            .is_empty()
    );
    gate.release();
    gate.release();
    hito.plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: 1,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn plugin_statuses_map_to_accurate_user_messages_without_leaking_the_body() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-status-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let not_responding = "The music service is not responding right now. Try again in a moment.";
    let busy = "The music service is busy. Try again in a moment.";
    let refused =
        "The plugin service refused the request. Check the connection in Settings → Plugins.";
    let rejected = "The plugin service rejected the request";
    for (status, expected) in [
        (StatusCode::BAD_GATEWAY, not_responding),
        (StatusCode::SERVICE_UNAVAILABLE, not_responding),
        (StatusCode::GATEWAY_TIMEOUT, not_responding),
        (StatusCode::TOO_MANY_REQUESTS, busy),
        (StatusCode::UNAUTHORIZED, refused),
        (StatusCode::FORBIDDEN, refused),
        (StatusCode::BAD_REQUEST, rejected),
        (StatusCode::INTERNAL_SERVER_ERROR, rejected),
    ] {
        server
            .response(
                status,
                HeaderMap::new(),
                b"private upstream error".to_vec(),
                false,
            )
            .await;
        let error = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id.clone(),
                    service_revision: service.revision,
                    bearer_token: None,
                },
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.to_string(), expected, "status {status}");
    }
}

#[path = "fixtures/plugin_service.rs"]
mod fixture;
mod support;

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
                    service_id: service.id.clone(),
                    service_revision: service.revision,
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

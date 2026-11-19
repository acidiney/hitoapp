use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn health_states_the_api_levels_an_app_needs_before_signing_in() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let services = hito_core::open(hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("health-levels").unwrap(),
    ))
    .await
    .unwrap();
    let app = hito_server::router_with_hosts(
        services,
        hito_server::Config::default(),
        None,
        hito_server::Hosts::default(),
    );
    let response = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["ready"], true);
    assert_eq!(body["api_level"], hito_server::API_LEVEL);
    assert_eq!(body["min_client_api_level"], hito_server::MIN_CLIENT_API_LEVEL);
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert!(hito_server::MIN_CLIENT_API_LEVEL >= 1);
    assert!(hito_server::API_LEVEL >= hito_server::MIN_CLIENT_API_LEVEL);
}

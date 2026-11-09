use crate::support::{admin, call, services};
use axum::http::StatusCode;
use hito_core::test_support::health_service::HealthServer;
use hito_core::{InstallPlugin, OfferPluginService};
use hito_plugin_sdk::{HealthReason, HealthReport, HealthState};
use serde_json::{Value, json};

const PLUGINS: &str = "/api/v1/plugins";

async fn connect(
    hito: &hito_core::Services,
    token: &str,
    server: &HealthServer,
) -> hito_core::PluginInstallation {
    let actor = hito.identity.authenticate(token).await.unwrap();
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Owned health service".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn the_plugin_list_reports_health_for_the_signed_in_account_only() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let server = HealthServer::start(
        "Owned health service",
        HealthReport {
            state: HealthState::Degraded,
            reason: Some(HealthReason::Network),
            since: Some(1_700_000_000),
        },
    )
    .await;
    connect(&hito, &token, &server).await;

    assert_eq!(
        call(&app, "GET", PLUGINS, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert!(server.health_requests().is_empty());
    let (status, settings) = call(&app, "GET", PLUGINS, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        settings["installations"][0]["health"],
        json!({"state": "degraded", "reason": "network", "since": 1_700_000_000})
    );
    assert_eq!(server.health_requests().len(), 1);
}

#[tokio::test]
async fn a_legacy_plugin_has_no_health_field_in_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let server = HealthServer::without_health("Legacy health service").await;
    connect(&hito, &token, &server).await;

    let (status, settings) = call(&app, "GET", PLUGINS, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(settings["installations"][0].get("health").is_none());
}

use axum::http::StatusCode;
use hito_core::test_support::health_service::HealthServer;
use hito_core::test_support::support;
use hito_core::{
    Actor, Credentials, InstallPlugin, OfferPluginService, PluginHealth, PluginHealthState,
    PluginInstallation, RefreshPlugin, Services,
};
use hito_plugin_sdk::{HealthReason, HealthReport, HealthState};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const SINCE: u64 = 1_700_000_000;

fn degraded(reason: HealthReason) -> HealthReport {
    HealthReport {
        state: HealthState::Degraded,
        reason: Some(reason),
        since: Some(SINCE),
    }
}

async fn connect(
    hito: &Services,
    actor: &Actor,
    server: &HealthServer,
    token: Option<&str>,
) -> PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: format!("Service {}", uuid::Uuid::new_v4()),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: token.map(str::to_owned),
            },
        )
        .await
        .unwrap()
}

async fn recheck(hito: &Services, actor: &Actor) {
    let current = hito.plugins.settings(actor).await.unwrap().installations;
    hito.plugins
        .refresh(
            actor,
            &current[0].id,
            RefreshPlugin {
                revision: current[0].revision,
                accept_manifest_digest: None,
                credential: Default::default(),
            },
        )
        .await
        .unwrap();
}

async fn health(hito: &Services, actor: &Actor) -> Option<PluginHealth> {
    let settings = hito.plugins.settings(actor).await.unwrap();
    assert_eq!(settings.installations.len(), 1);
    settings.installations.into_iter().next().unwrap().health
}

#[tokio::test]
async fn a_degraded_plugin_is_reported_on_its_installation_with_its_reason_and_since_time() {
    let server = HealthServer::start("Owned video service", degraded(HealthReason::Network)).await;
    server.require_bearer("private-health-token");
    let (_dir, hito, actor, _library, _music) = support::library("plugin-health-degraded").await;
    connect(&hito, &actor, &server, Some("private-health-token")).await;

    let reported = health(&hito, &actor).await.expect("health is reported");
    assert_eq!(reported.state, PluginHealthState::Degraded);
    assert_eq!(reported.reason, Some(HealthReason::Network));
    assert_eq!(reported.since, Some(SINCE));
    let requests = server.health_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].headers["x-hito-plugin-protocol"], "1");
    assert!(requests[0].headers.get("cookie").is_none());
    assert_eq!(requests[0].body, Value::Null);
}

#[tokio::test]
async fn a_healthy_plugin_reports_ok_without_a_reason() {
    let server = HealthServer::start("Owned video service", HealthReport::ok()).await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-health-ok").await;
    connect(&hito, &actor, &server, None).await;

    let reported = health(&hito, &actor).await.unwrap();
    assert_eq!(reported.state, PluginHealthState::Ok);
    assert_eq!(reported.reason, None);
    assert_eq!(reported.since, None);
    let wire = serde_json::to_value(reported).unwrap();
    assert_eq!(wire, json!({"state": "ok", "reason": null, "since": null}));
}

#[tokio::test]
async fn a_plugin_without_the_endpoint_reports_nothing_and_the_field_is_omitted() {
    let server = HealthServer::without_health("Legacy service").await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-health-legacy").await;
    connect(&hito, &actor, &server, None).await;

    let settings = hito.plugins.settings(&actor).await.unwrap();
    assert!(settings.installations[0].health.is_none());
    let wire = serde_json::to_value(&settings.installations[0]).unwrap();
    assert!(wire.get("health").is_none());
}

#[tokio::test]
async fn a_plugin_that_cannot_answer_in_time_is_reported_as_not_responding() {
    let server = HealthServer::start("Slow service", HealthReport::ok()).await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-health-timeout").await;
    connect(&hito, &actor, &server, None).await;
    server.delay(Duration::from_secs(6));

    let started = Instant::now();
    let reported = health(&hito, &actor).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_eq!(reported.state, PluginHealthState::Unreachable);
    assert_eq!(reported.reason, None);
    assert_eq!(reported.since, None);
}

#[tokio::test]
async fn a_plugin_that_is_gone_or_failing_is_reported_as_not_responding() {
    let mut server = HealthServer::start("Owned video service", HealthReport::ok()).await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-health-gone").await;
    connect(&hito, &actor, &server, None).await;
    server.status(StatusCode::BAD_GATEWAY);
    assert_eq!(
        health(&hito, &actor).await.unwrap().state,
        PluginHealthState::Unreachable
    );
    assert!(server.finish().await);
}

#[tokio::test]
async fn refusals_and_invalid_answers_report_nothing_instead_of_a_false_alarm() {
    let server = HealthServer::start("Owned video service", HealthReport::ok()).await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-health-invalid").await;
    connect(&hito, &actor, &server, None).await;
    for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
        server.status(status);
        assert!(health(&hito, &actor).await.is_none());
        recheck(&hito, &actor).await;
    }
    for value in [
        json!({"state": "ok", "reason": "network", "since": null}),
        json!({"state": "degraded", "reason": "network", "since": null}),
        json!({"state": "worse", "reason": null, "since": null}),
        json!({"state": "ok", "reason": null, "since": null, "title": "private"}),
    ] {
        server.raw(value);
        assert!(health(&hito, &actor).await.is_none());
        recheck(&hito, &actor).await;
    }
}

#[tokio::test]
async fn answers_are_cached_per_installation_so_settings_does_not_hammer_the_plugin() {
    let server = HealthServer::start("Owned video service", degraded(HealthReason::Refused)).await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-health-cache").await;
    connect(&hito, &actor, &server, None).await;

    for _ in 0..4 {
        assert_eq!(
            health(&hito, &actor).await.unwrap().reason,
            Some(HealthReason::Refused)
        );
    }
    assert_eq!(server.health_requests().len(), 1);

    recheck(&hito, &actor).await;
    server.report(HealthReport::ok());
    assert_eq!(
        health(&hito, &actor).await.unwrap().state,
        PluginHealthState::Ok
    );
    assert_eq!(server.health_requests().len(), 2);
}

#[tokio::test]
async fn another_account_never_triggers_or_sees_a_health_read_it_does_not_own() {
    let server = HealthServer::start("Owned video service", degraded(HealthReason::Network)).await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-health-account").await;
    connect(&hito, &actor, &server, None).await;
    hito.identity
        .create_user(
            &actor,
            Credentials {
                username: "health-guest".into(),
                password: "owned guest health password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "health-guest".into(),
            password: "owned guest health password".into(),
        })
        .await
        .unwrap();
    let guest = hito.identity.authenticate(&session.token).await.unwrap();

    let settings = hito.plugins.settings(&guest).await.unwrap();
    assert!(settings.installations.is_empty());
    assert!(server.health_requests().is_empty());
}

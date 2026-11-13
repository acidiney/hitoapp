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

#[path = "health_deadline_fixture.rs"]
mod deadline_fixture;

const SINCE: u64 = 1_700_000_000;

#[tokio::test]
async fn post_read_health_validation_cannot_outlive_the_settings_budget() {
    let mut case = deadline_fixture::Case::new().await;
    let body = case.server.stall_body();
    let started = tokio::time::Instant::now();
    let setup_deadline = started + Duration::from_secs(1);
    let body_deadline = started + Duration::from_millis(1_500);
    let response_deadline = started + Duration::from_millis(4_750);
    let (plugins, actor) = (case.hito.plugins.clone(), case.actor.clone());
    let mut request = tokio::spawn(async move {
        let answer = plugins.settings(&actor).await;
        (tokio::time::Instant::now(), answer)
    });
    let preparation = tokio::time::timeout_at(setup_deadline, async {
        body.entered().await;
        case.owner.reserve_all().await
    })
    .await;
    let reservation = match preparation {
        Ok(Ok(held)) => Some(held),
        _ => None,
    };
    let prepared = reservation.is_some();
    body.open();
    let body_retired = tokio::time::timeout_at(body_deadline, body.retired())
        .await
        .is_ok();
    let listener_joined = case.server.finish().await;
    let requests = case.server.health_requests();
    let observed = tokio::time::timeout_at(response_deadline, &mut request).await;
    let observed_before_release = observed.is_ok();
    if let Some(held) = reservation {
        held.release().await;
    }
    let joined = match observed {
        Ok(joined) => Some(joined),
        Err(_) => match tokio::time::timeout(Duration::from_secs(3), &mut request).await {
            Ok(joined) => Some(joined),
            Err(_) => {
                request.abort();
                let _ = request.await;
                None
            }
        },
    };
    let owner_connections = case.hito.plugins.connections(&case.actor).await;
    let peer_connections = case.hito.plugins.connections(&case.peer).await;
    let expected_id = case.installed.id.clone();
    let expected_revision = case.installed.revision;
    let expected_credential_set = case.installed.credential_set;
    let peer_id = case.peer_installation.clone();
    let cleanup = case.close().await;

    assert!(
        prepared,
        "fixture failed to reserve the pool before one second"
    );
    assert!(
        body_retired,
        "fixture body did not retire before the transport deadline"
    );
    assert!(listener_joined, "the owned health listener did not join");
    assert!(cleanup, "owned Core and SQLite resources did not retire");
    let connections = owner_connections
        .expect("owner connection recovery")
        .installations;
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].id, expected_id);
    assert_eq!(connections[0].revision, expected_revision);
    assert_eq!(connections[0].credential_set, expected_credential_set);
    let peer = peer_connections
        .expect("peer connection recovery")
        .installations;
    assert_eq!(peer.len(), 1);
    assert_eq!(peer[0].id, peer_id);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].headers["x-hito-plugin-protocol"], "1");
    assert!(requests[0].headers.get("cookie").is_none());
    assert_eq!(requests[0].body, Value::Null);
    let (completed, answer) = joined
        .expect("settings request must join")
        .expect("settings task");
    eprintln!(
        "health_deadline_oracle prepared={prepared} body_retired={body_retired} listener_joined={listener_joined} cleanup={cleanup} observed_before_release={observed_before_release} elapsed_ms={}",
        completed.duration_since(started).as_millis()
    );
    assert!(
        observed_before_release && completed <= response_deadline,
        "optional health validation exceeded its settings budget while SQLite was occupied"
    );
    let settings = answer.expect("settings result");
    assert_eq!(settings.installations.len(), 1);
    let installation = &settings.installations[0];
    assert_eq!(installation.id, expected_id);
    assert_eq!(installation.revision, expected_revision);
    assert_eq!(installation.credential_set, expected_credential_set);
    assert!(
        installation.health.is_none(),
        "unvalidated optional health must be omitted"
    );
}

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
    server.status(StatusCode::UNAUTHORIZED);
    let reported = health(&hito, &actor).await.unwrap();
    assert_eq!(reported.state, PluginHealthState::CredentialsRequired);
    assert_eq!(reported.reason, None);
    assert_eq!(reported.since, None);
    recheck(&hito, &actor).await;
    server.status(StatusCode::FORBIDDEN);
    assert!(health(&hito, &actor).await.is_none());
    recheck(&hito, &actor).await;
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

#[tokio::test]
async fn a_cached_health_answer_is_withheld_when_its_final_validation_cannot_finish_within_the_shared_budget()
 {
    let mut case = deadline_fixture::Case::new().await;
    let original_id = case.installed.id.clone();
    let peer_id = case.peer_installation.clone();
    let expected_original = serde_json::to_value(&case.installed);
    let peer_before = case.hito.plugins.connections(&case.peer).await;
    let primed = case.hito.plugins.settings(&case.actor).await;
    let primed_at = tokio::time::Instant::now();
    let original_requests_before = case.server.health_requests();
    let mut barrier_server =
        HealthServer::start("Owned cache barrier service", HealthReport::ok()).await;
    barrier_server.require_bearer("owned-cache-barrier-credential");
    let added: hito_core::Result<PluginInstallation> = async {
        let service = case
            .hito
            .plugins
            .offer_service(
                &case.actor,
                OfferPluginService {
                    name: "Owned cache barrier service".into(),
                    base_url: barrier_server.url.clone(),
                },
            )
            .await?;
        case.hito
            .plugins
            .install(
                &case.actor,
                InstallPlugin {
                    service_id: service.id,
                    service_revision: service.revision,
                    accept_manifest_digest: None,
                    bearer_token: Some("owned-cache-barrier-credential".into()),
                },
            )
            .await
    }
    .await;
    let expected_added = added.as_ref().ok().map(serde_json::to_value);
    let added_id = added
        .as_ref()
        .ok()
        .map(|installation| installation.id.clone());
    let body = barrier_server.stall_body();
    let started = tokio::time::Instant::now();
    let setup_deadline = started + Duration::from_secs(1);
    let body_deadline = started + Duration::from_millis(1_500);
    let response_deadline = started + Duration::from_millis(4_750);
    let (plugins, actor) = (case.hito.plugins.clone(), case.actor.clone());
    let mut request = tokio::spawn(async move {
        let answer = plugins.settings(&actor).await;
        (tokio::time::Instant::now(), answer)
    });
    let preparation = tokio::time::timeout_at(setup_deadline, async {
        body.entered().await;
        case.owner.reserve_all().await
    })
    .await;
    let reservation = match preparation {
        Ok(Ok(held)) => Some(held),
        _ => None,
    };
    let prepared = reservation.is_some() && tokio::time::Instant::now() <= setup_deadline;
    body.open();
    let body_retired = tokio::time::timeout_at(body_deadline, body.retired())
        .await
        .is_ok()
        && tokio::time::Instant::now() <= body_deadline;
    let barrier_listener_joined = barrier_server.finish().await;
    let barrier_requests = barrier_server.health_requests();
    let observed = tokio::time::timeout_at(response_deadline, &mut request).await;
    let observed_before_release = observed.is_ok();
    let original_requests_during = case.server.health_requests();
    if let Some(held) = reservation {
        held.release().await;
    }
    let joined = match observed {
        Ok(joined) => Some(joined),
        Err(_) => match tokio::time::timeout(Duration::from_secs(3), &mut request).await {
            Ok(joined) => Some(joined),
            Err(_) => {
                request.abort();
                let _ = request.await;
                None
            }
        },
    };
    let recovered = case.hito.plugins.settings(&case.actor).await;
    let recovered_at = tokio::time::Instant::now();
    let original_requests_after = case.server.health_requests();
    let owner_after = case.hito.plugins.connections(&case.actor).await;
    let peer_after = case.hito.plugins.connections(&case.peer).await;
    let original_listener_joined = case.server.finish().await;
    let cleanup = case.close().await;

    assert!(
        prepared,
        "fixture failed to reserve the pool before one second"
    );
    assert!(
        body_retired,
        "fixture body did not retire before the transport deadline"
    );
    assert!(
        barrier_listener_joined,
        "the owned barrier listener did not join"
    );
    assert!(
        original_listener_joined,
        "the owned original listener did not join"
    );
    assert!(cleanup, "owned Core and SQLite resources did not retire");
    assert!(
        recovered_at.duration_since(primed_at) < Duration::from_secs(30),
        "fixture exceeded the original cache freshness window"
    );
    let expected_original = expected_original.expect("original metadata snapshot");
    let expected_added = expected_added
        .expect("barrier installation must succeed")
        .expect("barrier metadata snapshot");
    let added_id = added_id.expect("barrier installation identity");
    let mut expected_owner = vec![expected_original.clone(), expected_added];
    expected_owner.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let expected_owner = Value::Array(expected_owner);
    let metadata = |mut installations: Vec<PluginInstallation>| {
        for installation in &mut installations {
            installation.health = None;
        }
        installations.sort_by(|left, right| left.id.cmp(&right.id));
        serde_json::to_value(installations).expect("installation metadata snapshot")
    };
    let expected_report = Some(PluginHealth {
        state: PluginHealthState::Degraded,
        reason: Some(HealthReason::Network),
        since: Some(1_700_000_017),
    });
    let primed = primed.expect("initial owner health read");
    assert_eq!(
        primed.installations.len(),
        1,
        "fixture must prime only the original connection"
    );
    assert_eq!(primed.installations[0].id, original_id);
    assert_eq!(primed.installations[0].health, expected_report);
    assert_eq!(
        metadata(primed.installations),
        Value::Array(vec![expected_original])
    );
    assert_eq!(
        original_requests_before.len(),
        1,
        "fixture did not prime exactly one health read"
    );
    assert_eq!(
        original_requests_during.len(),
        1,
        "the mixed read must use the original cached answer"
    );
    assert_eq!(
        original_requests_after.len(),
        1,
        "recovery must preserve the original cached answer"
    );
    assert_eq!(barrier_requests.len(), 1);
    assert_eq!(barrier_requests[0].headers["x-hito-plugin-protocol"], "1");
    assert!(barrier_requests[0].headers.get("cookie").is_none());
    assert_eq!(barrier_requests[0].body, Value::Null);
    let recovered = recovered.expect("owner settings recovery");
    let restored = recovered
        .installations
        .iter()
        .find(|installation| installation.id == original_id)
        .expect("original connection must survive recovery");
    assert_eq!(restored.health, expected_report);
    assert_eq!(metadata(recovered.installations), expected_owner);
    assert_eq!(
        metadata(
            owner_after
                .expect("owner connection recovery")
                .installations
        ),
        expected_owner
    );
    let peer_before = peer_before.expect("initial peer connections");
    assert_eq!(peer_before.installations.len(), 1);
    assert_eq!(peer_before.installations[0].id, peer_id);
    assert_ne!(peer_id, original_id);
    let peer_after = peer_after.expect("peer connection recovery");
    assert_eq!(
        metadata(peer_after.installations),
        metadata(peer_before.installations)
    );
    let (completed, answer) = joined
        .expect("mixed settings request must join")
        .expect("settings task");
    let settings = answer.expect("mixed settings result");
    let cached = settings
        .installations
        .iter()
        .find(|installation| installation.id == original_id);
    let cached_outcome = match cached {
        Some(installation) if installation.health.is_none() => "omitted",
        Some(installation) if installation.health == expected_report => "expected_report",
        Some(_) => "unexpected_report",
        None => "missing_installation",
    };
    eprintln!(
        "cached_health_deadline_oracle prepared={prepared} body_retired={body_retired} barrier_listener_joined={barrier_listener_joined} original_listener_joined={original_listener_joined} cleanup={cleanup} observed_before_release={observed_before_release} cached_outcome={cached_outcome} elapsed_ms={} original_reads_before={} original_reads_during={} original_reads_after={}",
        completed.duration_since(started).as_millis(),
        original_requests_before.len(),
        original_requests_during.len(),
        original_requests_after.len()
    );
    assert!(
        observed_before_release && completed <= response_deadline,
        "cached optional health validation exceeded the shared settings budget while SQLite was occupied"
    );
    let cached = cached.expect("mixed result must preserve the original installation");
    assert!(
        cached.health.is_none() || cached.health == expected_report,
        "cached health must be absent or match the validated original report"
    );
    let barrier = settings
        .installations
        .iter()
        .find(|installation| installation.id == added_id)
        .expect("mixed result must preserve the barrier installation");
    assert!(
        barrier.health.is_none(),
        "the blocked uncached report must be omitted"
    );
    assert_eq!(metadata(settings.installations), expected_owner);
}

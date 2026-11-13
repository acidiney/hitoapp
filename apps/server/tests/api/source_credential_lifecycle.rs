use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_plugin_sdk::{
    Capability, HEALTH_PATH, HealthReport, MANIFEST_PATH, Manifest, PROTOCOL_VERSION, SEARCH_PATH,
    SearchPage, SearchRequest,
};
use hito_testkit::fake_plugin::{FakePlugin, Gate, RecordedRequest, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::source_credential_fixture as fixture;
use fixture::{Context, NEW_TOKEN, OWNER_TOKEN, PEER_TOKEN, Response, account, installation_by_id};

#[derive(Clone, Copy)]
enum Scenario {
    NewerLyricSuccess,
    TargetReset,
    ConnectionWithdrawal,
}

#[tokio::test]
async fn an_older_held_source401_cannot_replace_a_newer_checked_lyric_success() {
    run(Scenario::NewerLyricSuccess).await;
}

#[tokio::test]
async fn a_source_success_held_across_target_reset_neither_clears_diagnosis_nor_publishes_the_old_target()
 {
    run(Scenario::TargetReset).await;
}

#[tokio::test]
async fn withdrawing_a_connection_while_source401_is_held_does_not_poison_its_replacement() {
    run(Scenario::ConnectionWithdrawal).await;
}

fn bearer_is(request: &RecordedRequest, token: &str) -> bool {
    request
        .headers
        .get("authorization")
        .is_some_and(|header| header == format!("Bearer {token}").as_str())
}

fn empty_search(request: &RecordedRequest) -> Reply {
    assert!(
        bearer_is(request, OWNER_TOKEN)
            || bearer_is(request, NEW_TOKEN)
            || bearer_is(request, PEER_TOKEN)
    );
    let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
    assert!(search.validate().is_ok());
    assert!(search.cursor.is_none());
    let page = SearchPage {
        items: Vec::new(),
        next_cursor: None,
    };
    assert!(page.validate(search.limit).is_ok());
    Reply::Json(serde_json::to_value(page).unwrap())
}

async fn run(scenario: Scenario) {
    let primary = fixture::ProviderServer::start().await;
    let previous = fixture::ProviderServer::start().await;
    let mut plugin = FakePlugin::builder(Manifest {
        id: "org.hito.source-lifecycle-fixture".into(),
        name: "Owned source lifecycle service".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [
            Capability::Search,
            Capability::Sources,
            Capability::Metadata,
            Capability::Lyrics,
        ]
        .into(),
        authorization: None,
        content_policy: None,
    })
    .require_protocol()
    .get(HEALTH_PATH, |_| {
        Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
    })
    .post(SEARCH_PATH, |request| {
        assert!(bearer_is(request, OWNER_TOKEN));
        let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
        assert!(search.validate().is_ok());
        assert!(
            search.query == "Owned performer Owned tune"
                && search.cursor.is_none()
                && search.limit == 20
        );
        Reply::Status(StatusCode::UNAUTHORIZED)
    })
    .start()
    .await;
    plugin.set_hold_limit(Duration::from_secs(4));
    let mut requests = JoinSet::new();
    let mut held = None;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise(
            scenario,
            &primary,
            &previous,
            &plugin,
            &mut requests,
            &mut held,
        )))
        .catch_unwind(),
    )
    .await;
    if let Some(gate) = held {
        gate.open();
    }
    requests.abort_all();
    tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await
    .unwrap();
    let stopped = plugin.finish().await;
    primary.shutdown().await;
    previous.shutdown().await;
    assert!(stopped);
    let (resolution, current) = match result {
        Ok(Ok(result)) => result,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned source lifecycle must settle within its deadline"),
    };
    let (outcome, health) = match scenario {
        Scenario::NewerLyricSuccess => ("provider_unavailable", "ok"),
        Scenario::TargetReset => ("cancelled", "credentials_required"),
        Scenario::ConnectionWithdrawal => ("provider_unavailable", "ok"),
    };
    assert_eq!(
        resolution["outcome"], outcome,
        "The old source receipt must retain its truthful completion"
    );
    assert_eq!(
        current["health"]["state"], health,
        "Only current, correctly ordered protected observations may affect this connection"
    );
}

async fn exercise(
    scenario: Scenario,
    primary: &fixture::ProviderServer,
    previous: &fixture::ProviderServer,
    plugin: &FakePlugin,
    requests: &mut JoinSet<Response>,
    held: &mut Option<std::sync::Arc<Gate>>,
) -> (Value, Value) {
    let context = Context::prepare(primary, previous, plugin).await;
    let started = Instant::now();
    let mut first = None;
    if matches!(scenario, Scenario::TargetReset) {
        let request = json!({"request_id":Uuid::new_v4()});
        let response = context.resolve(&request).await;
        context.check_resolution(&response, &request);
        assert_eq!(response.2["outcome"], "provider_unavailable");
        let diagnosed = context.settings().await;
        assert_eq!(
            installation_by_id(&diagnosed, &context.installation["id"])["health"]["state"],
            "credentials_required"
        );
        assert_eq!(context.settings().await, diagnosed);
        first = Some((request, response.2));
        plugin.on(SEARCH_PATH, |request| {
            assert_eq!(request.json()["query"], "Owned performer Owned tune");
            empty_search(request)
        });
    }
    let request = json!({"request_id":Uuid::new_v4()});
    let gate = plugin.hold_next(SEARCH_PATH);
    *held = Some(gate.clone());
    let app = context.app.clone();
    let path = context.path();
    let headers = context.owner.headers();
    let pending_request = request.clone();
    requests.spawn(async move {
        account::exchange(&app, "POST", &path, pending_request, headers).await
    });
    assert!(gate.entered_within(Duration::from_secs(1)).await);
    assert!(!gate.has_retired());
    assert!(requests.try_join_next().is_none());
    let expected_searches = if first.is_some() { 2 } else { 1 };
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), expected_searches);
    let mut expected_settings = context.before.clone();
    let mut expected_choice = context.choice.clone();
    let mut current_id = context.installation["id"].clone();
    match scenario {
        Scenario::NewerLyricSuccess => {
            plugin.on(SEARCH_PATH, |request| {
                assert_eq!(request.json()["query"], "Owned newer lyric recovery query");
                empty_search(request)
            });
            let (status, headers, success) = account::exchange(&context.app, "POST",
                &format!("/api/v1/recordings/{}/lyrics/candidates", context.owned.recording),
                json!({"installation_id":context.installation["id"], "installation_revision":context.installation["revision"], "query":"Owned newer lyric recovery query"}),
                context.owner.headers()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers["cache-control"], "private, no-store");
            assert_eq!(success, json!({"items":[],"next_cursor":null}));
            assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 2);
            assert_eq!(context.settings().await, context.before);
            assert!(!gate.has_retired());
            assert!(requests.try_join_next().is_none());
        }
        Scenario::TargetReset => {
            let (status, headers, reset) = account::exchange(&context.app, "POST",
                &format!("/api/v1/recordings/{}/source-choice/reset", context.owned.recording),
                json!({"request_id":Uuid::new_v4(), "expected_revision":context.choice["revision"]}), context.owner.headers()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers["cache-control"], "private, no-store");
            assert_eq!(reset["revision"], 1);
            expected_choice = context
                .owner
                .get(
                    &context.app,
                    &format!(
                        "/api/v1/recordings/{}/source-choice",
                        context.owned.recording
                    ),
                )
                .await;
            assert_eq!(expected_choice["mode"], "automatic");
            assert_eq!(expected_choice["revision"], 1);
            assert!(expected_choice["sources"].as_array().unwrap().is_empty());
            assert_eq!(
                installation_by_id(&context.settings().await, &current_id)["health"]["state"],
                "credentials_required"
            );
            assert!(!gate.has_retired());
            assert!(requests.try_join_next().is_none());
        }
        Scenario::ConnectionWithdrawal => {
            let (status, headers, removed) = account::exchange(
                &context.app,
                "DELETE",
                &format!(
                    "/api/v1/plugins/{}",
                    context.installation["id"].as_str().unwrap()
                ),
                json!({"revision":context.installation["revision"]}),
                context.owner.headers(),
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            assert_eq!(headers["cache-control"], "private, no-store");
            assert_eq!(removed, Value::Null);
            let withdrawn = context.settings().await;
            assert!(
                withdrawn["installations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|item| item["id"] != current_id)
            );
            assert_eq!(withdrawn["services"], context.before["services"]);
            assert!(!gate.has_retired());
            assert!(requests.try_join_next().is_none());
        }
    }
    gate.release();
    let response = tokio::time::timeout(Duration::from_secs(3), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(gate.retired_within(Duration::from_secs(1)).await);
    context.check_resolution(&response, &request);
    context.check_replay(&request, &response.2).await;
    if let Some((first_request, first_resolution)) = &first {
        context.check_replay(first_request, first_resolution).await;
    }
    if matches!(scenario, Scenario::ConnectionWithdrawal) {
        let withdrawn = context.settings().await;
        assert!(
            withdrawn["installations"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["id"] != current_id)
        );
        let service = context.before["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == context.installation["service_id"])
            .unwrap();
        let (status, headers, replacement) = account::exchange(&context.app, "POST", "/api/v1/plugins",
            json!({"service_id":service["id"], "service_revision":service["revision"], "bearer_token":NEW_TOKEN}), context.owner.headers()).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(headers["cache-control"], "private, no-store");
        assert_ne!(replacement["id"], context.installation["id"]);
        assert_eq!(
            replacement["service_id"],
            context.installation["service_id"]
        );
        assert_eq!(replacement["credential_set"], true);
        current_id = replacement["id"].clone();
        expected_settings = context.settings().await;
        assert_eq!(
            installation_by_id(&expected_settings, &current_id)["health"]["state"],
            "ok"
        );
        assert_eq!(expected_settings["services"], context.before["services"]);
        let manifests = plugin.requests_for(MANIFEST_PATH);
        assert_eq!(manifests.len(), 3);
        assert!(bearer_is(manifests.last().unwrap(), NEW_TOKEN));
        assert_eq!(plugin.health_requests().len(), 3);
        assert!(bearer_is(&plugin.health_requests()[2], NEW_TOKEN));
        context.check_replay(&request, &response.2).await;
        assert_eq!(context.settings().await, expected_settings);
    }
    let receipts = json!([response.2, first.as_ref().map(|(_, receipt)| receipt)]);
    let settings = context
        .check_preserved(
            &expected_settings,
            &expected_choice,
            primary,
            previous,
            plugin,
            &receipts,
        )
        .await;
    let current = installation_by_id(&settings, &current_id).clone();
    let withdrawal = matches!(scenario, Scenario::ConnectionWithdrawal);
    assert_eq!(
        plugin.health_requests().len(),
        if withdrawal { 3 } else { 2 }
    );
    assert_eq!(
        plugin.requests_for(MANIFEST_PATH).len(),
        if withdrawal { 3 } else { 2 }
    );
    assert_eq!(
        plugin.requests_for(SEARCH_PATH).len(),
        if withdrawal { 1 } else { 2 }
    );
    assert_eq!(plugin.requests().len(), 4);
    let operations: Vec<_> = plugin
        .requests()
        .into_iter()
        .map(|request| request.operation)
        .collect();
    let expected_operations = if withdrawal {
        vec![MANIFEST_PATH, MANIFEST_PATH, SEARCH_PATH, MANIFEST_PATH]
    } else {
        vec![MANIFEST_PATH, MANIFEST_PATH, SEARCH_PATH, SEARCH_PATH]
    };
    assert_eq!(operations, expected_operations);
    assert!(
        plugin
            .requests_for(SEARCH_PATH)
            .iter()
            .all(|request| bearer_is(request, OWNER_TOKEN))
    );
    assert!(requests.is_empty());
    assert!(started.elapsed() < Duration::from_secs(20));
    (response.2, current)
}

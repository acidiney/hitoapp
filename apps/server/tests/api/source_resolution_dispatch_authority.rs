use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_plugin_sdk::{
    AUDIO_PATH, DETAIL_PATH, MANIFEST_PATH, METADATA_PATH, SEARCH_PATH, SOURCES_PATH,
};
use hito_testkit::fake_plugin::{FakePlugin, Gate};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::source_authority_fixture as provider;
use crate::source_credential_fixture::{Context, OWNER_TOKEN, ProviderServer, Response, account};

#[tokio::test]
async fn resetting_a_held_nonempty_search_stops_all_later_source_discovery() {
    let primary = ProviderServer::start().await;
    let previous = ProviderServer::start().await;
    let mut first = provider::start("org.hito.source-authority-first", true, OWNER_TOKEN).await;
    let mut next = provider::start("org.hito.source-authority-next", false, OWNER_TOKEN).await;
    first.set_hold_limit(Duration::from_secs(4));
    let gate = first.hold_next(SEARCH_PATH);
    let mut requests = JoinSet::new();
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(exercise(
            &primary,
            &previous,
            &first,
            &next,
            &gate,
            &mut requests,
        ))
        .catch_unwind(),
    )
    .await;
    gate.open();
    requests.abort_all();
    tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await
    .unwrap();
    let later_requests = (
        [DETAIL_PATH, SOURCES_PATH, METADATA_PATH].map(|path| first.requests_for(path).len()),
        next.requests_for(SEARCH_PATH).len(),
    );
    let first_stopped = first.finish().await;
    let next_stopped = next.finish().await;
    primary.shutdown().await;
    previous.shutdown().await;
    assert!(first_stopped && next_stopped);
    let resolution = match result {
        Ok(Ok(resolution)) => resolution,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned source authority request must settle within its deadline"),
    };
    assert_eq!(resolution["outcome"], "cancelled");
    assert_eq!(
        later_requests,
        ([0, 0, 0], 0),
        "After the retained target changes, an old nonempty search must not dispatch detail, sources, metadata, or another provider search"
    );
}

async fn exercise(
    primary: &ProviderServer,
    previous: &ProviderServer,
    first: &FakePlugin,
    next: &FakePlugin,
    gate: &Gate,
    requests: &mut JoinSet<Response>,
) -> Value {
    let started = Instant::now();
    let mut context = Context::prepare(primary, previous, first).await;
    let next_installation = context
        .owner
        .connect(
            &context.app,
            &next.url,
            "Owned next source authority service",
            Some(OWNER_TOKEN),
        )
        .await;
    context
        .refresh_peer_baseline_after_offering(next_installation["service_id"].as_str().unwrap())
        .await;
    let before = context.settings().await;
    assert_eq!(
        before["installations"].as_array().unwrap().len(),
        context.before["installations"].as_array().unwrap().len() + 1
    );
    assert!(next.requests_for(SEARCH_PATH).is_empty());
    let request = json!({"request_id":Uuid::new_v4()});
    let app = context.app.clone();
    let headers = context.owner.headers();
    let path = context.path();
    let pending = request.clone();
    requests.spawn(async move { account::exchange(&app, "POST", &path, pending, headers).await });
    assert!(gate.entered_within(Duration::from_secs(1)).await);
    assert!(!gate.has_retired());
    assert!(requests.try_join_next().is_none());
    assert_eq!(first.requests_for(SEARCH_PATH).len(), 1);
    assert!(next.requests_for(SEARCH_PATH).is_empty());
    for operation in [DETAIL_PATH, SOURCES_PATH, METADATA_PATH] {
        assert!(first.requests_for(operation).is_empty());
    }
    let choice_path = format!(
        "/api/v1/recordings/{}/source-choice",
        context.owned.recording
    );
    let reset_id = Uuid::new_v4();
    let (status, headers, reset) = account::exchange(
        &context.app,
        "POST",
        &format!("{choice_path}/reset"),
        json!({"request_id":reset_id,"expected_revision":context.choice["revision"]}),
        context.owner.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(reset["request_id"], reset_id.to_string());
    assert_eq!(reset["recording_id"], context.owned.recording);
    assert_eq!(reset["revision"], 1);
    let reset_choice = context.owner.get(&context.app, &choice_path).await;
    assert_eq!(reset_choice["mode"], "automatic");
    assert_eq!(reset_choice["revision"], 1);
    assert!(reset_choice["sources"].as_array().unwrap().is_empty());
    assert!(!gate.has_retired());
    assert!(requests.try_join_next().is_none());
    gate.release();
    let response = requests.join_next().await.unwrap().unwrap();
    assert!(requests.is_empty());
    assert!(gate.has_retired());
    context.check_resolution(&response, &request);
    assert_eq!(response.2["outcome"], "cancelled");
    let before_replay = (
        first.requests().len(),
        next.requests().len(),
        first.health_requests().len(),
        next.health_requests().len(),
    );
    context.check_replay(&request, &response.2).await;
    assert_eq!(
        before_replay,
        (
            first.requests().len(),
            next.requests().len(),
            first.health_requests().len(),
            next.health_requests().len(),
        )
    );
    let after = context
        .check_preserved(
            &before,
            &reset_choice,
            primary,
            previous,
            first,
            &json!([reset, response.2]),
        )
        .await;
    assert_eq!(after, before);
    assert_eq!(first.requests_for(MANIFEST_PATH).len(), 2);
    assert_eq!(first.health_requests().len(), 2);
    assert_eq!(first.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(next.requests_for(MANIFEST_PATH).len(), 1);
    assert_eq!(next.health_requests().len(), 1);
    assert_eq!(first.unexpected(), 0);
    assert_eq!(next.unexpected(), 0);
    assert!(first.requests_for(AUDIO_PATH).is_empty());
    for operation in [DETAIL_PATH, SOURCES_PATH, METADATA_PATH, AUDIO_PATH] {
        assert!(next.requests_for(operation).is_empty());
    }
    let receipt = serde_json::to_string(&response.2).unwrap();
    assert!(!receipt.contains(next.url.as_str()));
    assert!(!receipt.contains("owned-authority-candidate"));
    assert!(!receipt.contains("owned-authority-source"));
    assert!(started.elapsed() < Duration::from_secs(20));
    response.2
}

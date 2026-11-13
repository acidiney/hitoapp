use crate::source_authority_snapshot_fixture::SnapshotFixture;
use crate::source_credential_fixture::{account, installation_by_id};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_plugin_sdk::{
    AUDIO_PATH, DETAIL_PATH, MANIFEST_PATH, METADATA_PATH, SEARCH_PATH, SOURCES_PATH,
};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_committed_during_the_authority_snapshot_prevents_protected_search_dispatch() {
    let mut fixture =
        SnapshotFixture::start("org.hito.snapshot-session-source", "source_exchange").await;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise(&mut fixture))).catch_unwind(),
    )
    .await;
    let (dispatched, _context) = fixture.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned snapshot-session workflow must settle within its deadline"),
    }
    assert_eq!(
        dispatched, 0,
        "A logout already committed during the read snapshot must prevent the next protected provider dispatch"
    );
}

async fn exercise(fixture: &mut SnapshotFixture) {
    let _request = fixture.begin().await;
    let SnapshotFixture {
        primary,
        previous,
        plugin,
        hold,
        requests,
        context,
        inspector,
        ready,
    } = fixture;
    let context = context.as_mut().unwrap();
    let inspector = inspector.take().unwrap();
    let ready = ready.as_ref().unwrap();
    hold.entered().await;
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    assert!(requests.try_join_next().is_none());
    let (status, _) = tokio::time::timeout(
        Duration::from_secs(2),
        context
            .owner
            .request(&context.app, "POST", "/api/v1/logout", Value::Null),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NO_CONTENT);
    hold.assert_held();
    let (status, _, rejected) = account::exchange(
        &context.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        context.owner.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(rejected.get("plugin_connection").is_none());
    hold.assert_held();
    assert!(requests.try_join_next().is_none());
    hold.release();
    let response = tokio::time::timeout(Duration::from_secs(2), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    hold.assert_returned();
    assert_eq!(response.0, StatusCode::UNAUTHORIZED);
    assert_eq!(response.1["cache-control"], "private, no-store");
    assert!(response.2.get("plugin_connection").is_none());
    assert!(response.2.get("code").is_none());
    for path in [DETAIL_PATH, SOURCES_PATH, METADATA_PATH, AUDIO_PATH] {
        assert!(plugin.requests_for(path).is_empty());
    }
    context.owner = inspector;
    assert_eq!(
        context.owner.get(&context.app, &ready.path).await,
        ready.before
    );
    let settings = context
        .check_preserved(
            &context.before,
            &context.choice,
            primary,
            previous,
            plugin,
            &response.2,
        )
        .await;
    assert_eq!(settings, context.before);
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.unexpected(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_committed_during_the_search_postcheck_preserves_credential_health() {
    let mut fixture =
        SnapshotFixture::start("org.hito.snapshot-postio-source", "source_search_postcheck").await;
    fixture.reject_search();
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise_postcheck(&mut fixture))).catch_unwind(),
    )
    .await;
    let (_, _context) = fixture.finish().await;
    let (current, expected) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned snapshot-session workflow must settle within its deadline"),
    };
    assert_eq!(
        current, expected,
        "A protected401 checked through a snapshot older than committed logout must not publish stale credential health"
    );
}

async fn exercise_postcheck(fixture: &mut SnapshotFixture) -> (Value, Value) {
    let _request = fixture.begin().await;
    let SnapshotFixture {
        primary,
        previous,
        plugin,
        hold,
        requests,
        context,
        inspector,
        ready,
    } = fixture;
    let context = context.as_mut().unwrap();
    let inspector = inspector.take().unwrap();
    let ready = ready.as_ref().unwrap();
    hold.entered().await;
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert!(requests.try_join_next().is_none());
    let (status, _) = tokio::time::timeout(
        Duration::from_secs(2),
        context
            .owner
            .request(&context.app, "POST", "/api/v1/logout", Value::Null),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NO_CONTENT);
    hold.assert_held();
    let (status, _, rejected) = account::exchange(
        &context.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        context.owner.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(rejected.get("plugin_connection").is_none());
    hold.assert_held();
    assert!(requests.try_join_next().is_none());
    hold.release();
    let response = tokio::time::timeout(Duration::from_secs(2), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    hold.assert_returned();
    assert_eq!(response.0, StatusCode::UNAUTHORIZED);
    assert_eq!(response.1["cache-control"], "private, no-store");
    assert!(response.2.get("plugin_connection").is_none());
    assert!(response.2.get("code").is_none());
    for path in [DETAIL_PATH, SOURCES_PATH, METADATA_PATH, AUDIO_PATH] {
        assert!(plugin.requests_for(path).is_empty());
    }
    context.owner = inspector;
    assert_eq!(
        context.owner.get(&context.app, &ready.path).await,
        ready.before
    );
    let settings = context
        .check_preserved(
            &context.before,
            &context.choice,
            primary,
            previous,
            plugin,
            &response.2,
        )
        .await;
    let current = installation_by_id(&settings, &context.installation["id"]).clone();
    let expected = installation_by_id(&context.before, &context.installation["id"]).clone();
    assert_eq!(expected["health"]["state"], "ok");
    let mut actual_without_owner_health = settings.clone();
    let owner = actual_without_owner_health["installations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["id"] == context.installation["id"])
        .unwrap();
    owner["health"] = expected["health"].clone();
    assert_eq!(actual_without_owner_health, context.before);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 2);
    assert_eq!(plugin.requests().len(), 3);
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.unexpected(), 0);
    (current, expected)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reset_committed_during_the_authority_snapshot_prevents_protected_search_dispatch() {
    let mut fixture =
        SnapshotFixture::start("org.hito.snapshot-choice-source", "source_exchange").await;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise_choice(&mut fixture))).catch_unwind(),
    )
    .await;
    let (dispatched, _context) = fixture.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned snapshot-session workflow must settle within its deadline"),
    }
    assert_eq!(
        dispatched, 0,
        "A source-choice reset already committed during the read snapshot must prevent the next protected provider dispatch"
    );
}

async fn exercise_choice(fixture: &mut SnapshotFixture) {
    let request = fixture.begin().await;
    let SnapshotFixture {
        primary,
        previous,
        plugin,
        hold,
        requests,
        context,
        inspector,
        ready,
    } = fixture;
    let context = context.as_mut().unwrap();
    let inspector = inspector.take().unwrap();
    let ready = ready.as_ref().unwrap();
    hold.entered().await;
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    assert!(requests.try_join_next().is_none());
    let choice_path = format!(
        "/api/v1/recordings/{}/source-choice",
        context.owned.recording
    );
    let reset_id = Uuid::new_v4();
    let (status, headers, reset) = tokio::time::timeout(
        Duration::from_secs(2),
        account::exchange(
            &context.app,
            "POST",
            &format!("{choice_path}/reset"),
            json!({"request_id":reset_id,"expected_revision":context.choice["revision"]}),
            context.owner.headers(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(
        reset,
        json!({
            "request_id":reset_id, "recording_id":context.owned.recording, "revision":1
        })
    );
    hold.assert_held();
    let reset_choice = tokio::time::timeout(
        Duration::from_secs(2),
        inspector.get(&context.app, &choice_path),
    )
    .await
    .unwrap();
    assert_eq!(
        reset_choice,
        json!({
            "recording_id":context.owned.recording, "revision":1, "mode":"automatic", "sources":[]
        })
    );
    hold.assert_held();
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    assert!(requests.try_join_next().is_none());
    hold.release();
    let response = tokio::time::timeout(Duration::from_secs(2), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    hold.assert_returned();
    context.check_resolution(&response, &request);
    assert_eq!(response.2["outcome"], "cancelled");
    assert_eq!(response.1["cache-control"], "private, no-store");
    assert!(response.2.get("plugin_connection").is_none());
    assert!(response.2.get("code").is_none());
    for path in [DETAIL_PATH, SOURCES_PATH, METADATA_PATH, AUDIO_PATH] {
        assert!(plugin.requests_for(path).is_empty());
    }
    context.owner = inspector;
    assert_eq!(
        context.owner.get(&context.app, &ready.path).await,
        ready.before
    );
    let settings = context
        .check_preserved(
            &context.before,
            &reset_choice,
            primary,
            previous,
            plugin,
            &json!([reset, response.2]),
        )
        .await;
    assert_eq!(settings, context.before);
    assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 2);
    assert_eq!(
        plugin.requests().len(),
        2 + plugin.requests_for(SEARCH_PATH).len()
    );
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.unexpected(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reset_committed_during_the_search_postcheck_preserves_credential_health() {
    let mut fixture = SnapshotFixture::start(
        "org.hito.snapshot-postio-choice-source",
        "source_search_postcheck",
    )
    .await;
    fixture.reject_search();
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise_choice_postcheck(&mut fixture))).catch_unwind(),
    )
    .await;
    let (_, _context) = fixture.finish().await;
    let (current, expected) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned snapshot-session workflow must settle within its deadline"),
    };
    assert_eq!(
        current, expected,
        "A protected401 checked through a snapshot older than a committed source-choice reset must not publish stale credential health"
    );
}

async fn exercise_choice_postcheck(fixture: &mut SnapshotFixture) -> (Value, Value) {
    let request = fixture.begin().await;
    let SnapshotFixture {
        primary,
        previous,
        plugin,
        hold,
        requests,
        context,
        inspector,
        ready,
    } = fixture;
    let context = context.as_mut().unwrap();
    let inspector = inspector.take().unwrap();
    let ready = ready.as_ref().unwrap();
    hold.entered().await;
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert!(requests.try_join_next().is_none());
    let choice_path = format!(
        "/api/v1/recordings/{}/source-choice",
        context.owned.recording
    );
    let reset_id = Uuid::new_v4();
    let (status, headers, reset) = tokio::time::timeout(
        Duration::from_secs(2),
        account::exchange(
            &context.app,
            "POST",
            &format!("{choice_path}/reset"),
            json!({"request_id":reset_id,"expected_revision":context.choice["revision"]}),
            context.owner.headers(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(
        reset,
        json!({
            "request_id":reset_id, "recording_id":context.owned.recording, "revision":1
        })
    );
    hold.assert_held();
    let reset_choice = tokio::time::timeout(
        Duration::from_secs(2),
        inspector.get(&context.app, &choice_path),
    )
    .await
    .unwrap();
    assert_eq!(
        reset_choice,
        json!({
            "recording_id":context.owned.recording, "revision":1, "mode":"automatic", "sources":[]
        })
    );
    hold.assert_held();
    assert!(requests.try_join_next().is_none());
    hold.release();
    let response = tokio::time::timeout(Duration::from_secs(2), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    hold.assert_returned();
    context.check_resolution(&response, &request);
    assert_eq!(response.2["outcome"], "cancelled");
    assert_eq!(response.1["cache-control"], "private, no-store");
    assert!(response.2.get("plugin_connection").is_none());
    assert!(response.2.get("code").is_none());
    for path in [DETAIL_PATH, SOURCES_PATH, METADATA_PATH, AUDIO_PATH] {
        assert!(plugin.requests_for(path).is_empty());
    }
    context.owner = inspector;
    assert_eq!(
        context.owner.get(&context.app, &ready.path).await,
        ready.before
    );
    let settings = context
        .check_preserved(
            &context.before,
            &reset_choice,
            primary,
            previous,
            plugin,
            &json!([reset, response.2]),
        )
        .await;
    let current = installation_by_id(&settings, &context.installation["id"]).clone();
    let expected = installation_by_id(&context.before, &context.installation["id"]).clone();
    assert_eq!(expected["health"]["state"], "ok");
    let mut actual_without_owner_health = settings.clone();
    let owner = actual_without_owner_health["installations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["id"] == context.installation["id"])
        .unwrap();
    owner["health"] = expected["health"].clone();
    assert_eq!(actual_without_owner_health, context.before);
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 2);
    assert_eq!(plugin.requests().len(), 3);
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.unexpected(), 0);
    (current, expected)
}

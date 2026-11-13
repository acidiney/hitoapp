use crate::{
    account::exchange,
    artist_identity_read_fence_fixture::{Fixture, Response, assert_denied},
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};

#[derive(Clone, Copy)]
enum Change {
    Logout,
    Lock,
    Withdraw,
}

struct Observed {
    response: Response,
    expected: StatusCode,
    before: Value,
    after: Value,
    counts: Value,
    current_counts: Value,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn group_list_does_not_return_a_snapshot_older_than_committed_logout() {
    run("artist_identity_groups_read", Change::Logout, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn group_detail_does_not_return_a_snapshot_older_than_committed_lock() {
    run("artist_identity_group_read", Change::Lock, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_surviving_group_does_not_return_recordings_from_a_withdrawn_library() {
    run("artist_identity_group_read", Change::Withdraw, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_current_read_does_not_return_a_snapshot_older_than_committed_logout() {
    run("credit_member_identity_read", Change::Logout, true).await;
}

async fn run(point: &'static str, change: Change, member: bool) {
    let mut fixture = Fixture::new(point);
    let outcome = AssertUnwindSafe(Box::pin(async {
        hang_guard(
            "owned two-library read fence setup",
            Box::pin(fixture.prepare()),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(&mut fixture, change, member),
        )
        .await
        .expect("The held public identity read must settle within its existing workflow deadline")
    }))
    .catch_unwind()
    .await;
    fixture.retire().await;
    let found = match outcome {
        Ok(found) => found,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(found.after, found.before);
    assert_eq!(found.current_counts, found.counts);
    assert_denied(&found.response, found.expected);
}

async fn exercise(fixture: &mut Fixture, change: Change, member: bool) -> Observed {
    if matches!(change, Change::Lock) {
        let ready = fixture.ready.as_ref().unwrap();
        assert_eq!(ready.owner.request(&ready.app, "PUT", "/api/v1/account/pin",
            json!({"password": "owned artist review administrator password", "pin": "1234", "idle_minutes": 0})).await.0,
            StatusCode::OK);
    }
    let ready = fixture.ready.as_ref().unwrap();
    let mut path = if member {
        ready.choose_member().await
    } else if matches!(change, Change::Logout) {
        "/api/v1/artist-identity-groups".into()
    } else {
        format!("/api/v1/artist-identity-groups/{}", ready.group)
    };
    let mut before = ready.snapshot().await;
    if member {
        before["member"] = ready.inspector.get(&ready.app, &path).await;
    }
    if matches!(change, Change::Withdraw) {
        let library_offset = before["detail"]["libraries"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .position(|item| item["library_id"] != ready.shared)
            .unwrap();
        let recording_offset = before["detail"]["recordings"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .position(|item| item["recording"]["id"] == ready.imported)
            .unwrap();
        path = format!(
            "{path}?library_offset={library_offset}&recording_offset={recording_offset}&limit=1"
        );
        let page = ready.owner.get(&ready.app, &path).await;
        assert_eq!(page["group"]["name"]["recording_id"], ready.imported);
        assert_eq!(page["group"]["library_count"], 2);
        assert_eq!(page["libraries"]["items"].as_array().unwrap().len(), 1);
        assert_ne!(page["libraries"]["items"][0]["library_id"], ready.shared);
        assert_eq!(page["recordings"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            page["recordings"]["items"][0]["recording"]["id"],
            ready.imported
        );
    }
    assert_eq!(before["detail"]["group"]["library_count"], 2);
    assert_eq!(before["detail"]["recordings"]["total"], 2);
    let counts = fixture.counts().await;
    fixture.begin(path.clone());
    fixture.hold.entered().await;
    assert!(fixture.requests.try_join_next().is_none());
    let ready = fixture.ready.as_ref().unwrap();
    let (method, target, expected) = match change {
        Change::Logout => ("POST", "/api/v1/logout".into(), StatusCode::UNAUTHORIZED),
        Change::Lock => ("POST", "/api/v1/account/lock".into(), StatusCode::LOCKED),
        Change::Withdraw => (
            "DELETE",
            format!(
                "/api/v1/libraries/{}/members/{}",
                ready.shared, ready.owner.id
            ),
            StatusCode::NOT_FOUND,
        ),
    };
    let changed = tokio::time::timeout(
        Duration::from_secs(2),
        ready
            .owner
            .request(&ready.app, method, &target, Value::Null),
    )
    .await
    .unwrap();
    assert_eq!(
        changed.0,
        if matches!(change, Change::Lock) {
            StatusCode::OK
        } else {
            StatusCode::NO_CONTENT
        }
    );
    fixture.hold.assert_held();
    if matches!(change, Change::Withdraw) {
        let current = ready
            .inspector
            .get(
                &ready.app,
                &format!("/api/v1/artist-identity-groups/{}", ready.group),
            )
            .await;
        assert_eq!(current["group"]["id"], ready.group);
        assert_eq!(current["group"]["library_count"], 1);
        assert_eq!(current["recordings"]["total"], 1);
        assert_eq!(
            current["recordings"]["items"][0]["recording"]["id"],
            ready.imported
        );
    }
    let probe = if matches!(change, Change::Withdraw) {
        format!("/api/v1/recordings/{}/metadata", ready.local)
    } else {
        "/api/v1/plugins".into()
    };
    let denied = exchange(
        &ready.app,
        "GET",
        &probe,
        Value::Null,
        ready.owner.headers(),
    )
    .await;
    assert_eq!(denied.0, expected);
    fixture.hold.assert_held();
    assert!(fixture.requests.try_join_next().is_none());
    fixture.hold.release();
    let response = tokio::time::timeout(Duration::from_secs(2), fixture.requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    fixture.hold.assert_returned();
    if matches!(change, Change::Withdraw) {
        assert_eq!(
            ready
                .inspector
                .request(&ready.app, "PUT", &target, Value::Null)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
    }
    let mut after = ready.snapshot().await;
    if member {
        after["member"] = ready.inspector.get(&ready.app, &path).await;
    }
    let current_counts = fixture.counts().await;
    Observed {
        response,
        expected,
        before,
        after,
        counts,
        current_counts,
    }
}

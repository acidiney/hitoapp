use crate::{
    account::exchange,
    credit_member_identity_reset_authority_fixture::{Fixture, Response},
    credit_member_identity_reset_fixture::accepted,
    credit_member_review_preview_fixture::private,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};

struct Observed {
    response: Response,
    probe: Response,
    before: Value,
    after: Value,
    counts: Value,
    current_counts: Value,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_member_reset_reports_account_deletion_after_its_first_fresh_session_check() {
    run(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_reset_replay_reports_account_deletion_after_its_first_fresh_session_check() {
    run(true).await;
}

async fn run(replay: bool) {
    let mut fixture = Fixture::new();
    let outcome = AssertUnwindSafe(Box::pin(async {
        hang_guard(
            "owned member reset authority setup",
            Box::pin(fixture.prepare()),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(30), exercise(&mut fixture, replay))
            .await
            .expect("The held member reset must settle within the existing workflow deadline")
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
    assert_eq!(found.probe.0, StatusCode::UNAUTHORIZED);
    private(&found.probe.1);
    assert_eq!(found.probe.2, json!({"error": "Sign in to continue"}));
    assert_eq!(
        found.response.0,
        StatusCode::UNAUTHORIZED,
        "A deleted account must receive HTTP 401 before a missing recording or grant can return HTTP 404"
    );
    private(&found.response.1);
    assert_eq!(found.response.2, json!({"error": "Sign in to continue"}));
}

async fn exercise(fixture: &mut Fixture, replay: bool) -> Observed {
    if replay {
        let owned = fixture.owned.as_ref().unwrap();
        let member = fixture.member.as_ref().unwrap();
        let receipt = accepted(
            &owned.app,
            &owned.peer,
            "POST",
            &format!("{}/reset", member.path),
            member.request.clone(),
        )
        .await;
        assert_eq!(receipt["request_id"], member.request["request_id"]);
        assert_eq!(receipt["revision"], 2);
        assert_eq!(receipt["selection"], Value::Null);
        let current = owned.peer.get(&owned.app, &member.path).await;
        assert_eq!(current["revision"], 2);
        assert_eq!(current["state"], json!({"status": "unselected"}));
    }
    let before = fixture.owned.as_ref().unwrap().snapshot().await;
    let counts = fixture.counts().await;
    fixture.begin_reset();
    fixture.hold.entered().await;
    assert!(fixture.requests.try_join_next().is_none());
    fixture.begin_delete();
    let deleted = tokio::time::timeout(Duration::from_secs(2), fixture.deletions.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        deleted.0,
        StatusCode::NO_CONTENT,
        "DELETE /api/v1/users/member returned HTTP {}",
        deleted.0
    );
    private(&deleted.1);
    assert_eq!(deleted.2, Value::Null);
    fixture.hold.assert_held();
    assert!(fixture.requests.try_join_next().is_none());
    let owned = fixture.owned.as_ref().unwrap();
    let probe = tokio::time::timeout(
        Duration::from_secs(2),
        exchange(
            &owned.app,
            "GET",
            "/api/v1/me",
            Value::Null,
            owned.peer.headers(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(probe.0, StatusCode::UNAUTHORIZED);
    fixture.hold.assert_held();
    assert!(fixture.requests.try_join_next().is_none());
    fixture.hold.release();
    let response = tokio::time::timeout(Duration::from_secs(2), fixture.requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    fixture.hold.assert_returned();
    let after = fixture.owned.as_ref().unwrap().snapshot().await;
    let current_counts = fixture.counts().await;
    assert_eq!(current_counts["unexpected"], 0);
    Observed {
        response,
        probe,
        before,
        after,
        counts,
        current_counts,
    }
}

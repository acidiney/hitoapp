#![cfg(target_os = "linux")]

#[path = "support/sqlite_clock.rs"]
mod sqlite_clock;

use std::{collections::BTreeSet, future::Future, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use hito_community::{Config, Service, router};
use serde_json::{Value, json};
use sqlite_clock::Clock;
use tower::ServiceExt;
use uuid::Uuid;

const ADMIN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";
const NOW: i64 = 1_900_000_000;

#[tokio::test]
async fn global_pending_limit_linearizes_concurrent_reporters_and_preserves_durable_receipts() {
    tokio::time::timeout(Duration::from_secs(45), global_admission())
        .await
        .expect("Owned global correction fixture exceeded 45 seconds");
}

async fn global_admission() {
    let clock = Clock::new(NOW);
    let fixture = tempfile::tempdir().unwrap();
    let config = || Config {
        state_directory: fixture.path().join("private-global-correction-limit-state"),
        admin_token: ADMIN.to_owned(),
    };
    let service = bounded(Service::open(config())).await.unwrap();
    let app = router(service.clone());
    let mut expected = BTreeSet::new();
    let mut first = None;
    let mut candidate = None;
    for reporter_index in 0..32 {
        let reporter = provision(&app).await;
        let count = if reporter_index == 31 { 31 } else { 32 };
        for index in 0..count {
            clock.set(NOW + reporter_index * 240 + index / 10 * 60);
            let claim = suggestion();
            let accepted = submit(&app, &reporter, claim.clone()).await;
            assert_eq!(accepted.status, StatusCode::CREATED);
            assert!(accepted.retry_after.is_none());
            assert!(expected.insert(accepted.body["correction_id"].as_str().unwrap().to_owned()));
            if reporter_index == 0 && index == 0 {
                first = Some((reporter.clone(), claim, accepted.body));
            }
        }
        candidate = Some(reporter);
    }
    let (first_reporter, first_claim, first_receipt) = first.unwrap();
    let candidate = candidate.unwrap();
    assert_eq!(expected.len(), 1023);
    assert_eq!(pending(&app).await, expected);
    bounded(service.close()).await;
    drop(app);
    drop(service);

    let reopened = bounded(Service::open(config())).await.unwrap();
    let app = router(reopened.clone());
    assert_eq!(pending(&app).await, expected);
    let peer = provision(&app).await;
    let left = suggestion();
    let right = suggestion();
    let (left_reply, right_reply) = tokio::join!(
        submit(&app, &candidate, left.clone()),
        submit(&app, &peer, right.clone())
    );
    let mut statuses = [left_reply.status, right_reply.status];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::CREATED, StatusCode::TOO_MANY_REQUESTS]
    );
    let (
        accepted_reporter,
        accepted_claim,
        accepted_receipt,
        blocked_reporter,
        blocked_claim,
        limited,
    ) = if left_reply.status == StatusCode::CREATED {
        (
            candidate,
            left,
            left_reply.body,
            peer.clone(),
            right,
            right_reply,
        )
    } else {
        (
            peer.clone(),
            right,
            right_reply.body,
            candidate,
            left,
            left_reply,
        )
    };
    assert_limited(limited);
    let accepted_id = accepted_receipt["correction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(expected.insert(accepted_id.clone()));
    assert_eq!(expected.len(), 1024);
    assert_eq!(pending(&app).await, expected);
    assert_limited(submit(&app, &blocked_reporter, blocked_claim.clone()).await);
    assert_replay(
        submit(&app, &accepted_reporter, accepted_claim.clone()).await,
        &accepted_receipt,
    );
    assert_replay(
        submit(&app, &first_reporter, first_claim.clone()).await,
        &first_receipt,
    );
    let mut conflict = first_claim.clone();
    conflict["left"]["recording_id"] = json!("43");
    let rejected = submit(&app, &first_reporter, conflict).await;
    assert_eq!(rejected.status, StatusCode::CONFLICT);
    assert_eq!(rejected.body, json!({"error": "submission_conflict"}));
    assert!(rejected.retry_after.is_none());
    let mut invalid = suggestion();
    invalid["left"]["recording_id"] = json!("042");
    let rejected = submit(&app, &blocked_reporter, invalid).await;
    assert_eq!(rejected.status, StatusCode::BAD_REQUEST);
    assert_eq!(rejected.body, json!({"error": "invalid_input"}));
    assert!(rejected.retry_after.is_none());
    assert_eq!(pending(&app).await, expected);

    let approved = request(
        &app,
        "POST",
        &format!("/v1/admin/corrections/{accepted_id}/review"),
        Some(ADMIN),
        Some(json!({"request_id": Uuid::new_v4(), "expected_revision": 1, "decision": "approve"})),
    )
    .await;
    assert_eq!(approved.status, StatusCode::CREATED);
    assert_eq!(approved.body["revision"], 2);
    assert!(expected.remove(&accepted_id));
    assert_eq!(pending(&app).await, expected);
    let unblocked = submit(&app, &blocked_reporter, blocked_claim.clone()).await;
    assert_eq!(unblocked.status, StatusCode::CREATED);
    assert!(unblocked.retry_after.is_none());
    assert_eq!(
        unblocked.body["submission_id"],
        blocked_claim["submission_id"]
    );
    assert!(expected.insert(unblocked.body["correction_id"].as_str().unwrap().to_owned()));
    assert_eq!(pending(&app).await, expected);
    assert_limited(submit(&app, &peer, suggestion()).await);
    assert_replay(
        submit(&app, &accepted_reporter, accepted_claim.clone()).await,
        &accepted_receipt,
    );
    let approved = request(
        &app,
        "GET",
        "/v1/admin/corrections?status=approved",
        Some(ADMIN),
        None,
    )
    .await;
    assert_eq!(approved.status, StatusCode::OK);
    assert_eq!(approved.body["suggestions"].as_array().unwrap().len(), 1);
    assert_eq!(
        approved.body["suggestions"][0]["correction_id"],
        accepted_id
    );
    assert_eq!(approved.body["suggestions"][0]["revision"], 2);
    bounded(reopened.close()).await;
    drop(app);
    drop(reopened);

    let final_service = bounded(Service::open(config())).await.unwrap();
    let app = router(final_service.clone());
    assert_eq!(pending(&app).await, expected);
    assert_limited(submit(&app, &peer, suggestion()).await);
    assert_replay(
        submit(&app, &blocked_reporter, blocked_claim).await,
        &unblocked.body,
    );
    assert_replay(
        submit(&app, &accepted_reporter, accepted_claim).await,
        &accepted_receipt,
    );
    assert_replay(
        submit(&app, &first_reporter, first_claim).await,
        &first_receipt,
    );
    assert!(
        clock.reads() >= 1025,
        "Real SQLite rows did not use the fixture clock"
    );
    bounded(final_service.close()).await;
}

fn suggestion() -> Value {
    json!({"submission_id": Uuid::new_v4(), "left": {"provider": "jamendo", "recording_id": "42"}, "right": {"provider": "youtube", "recording_id": "PrivTone001"}, "relation": "same_recording"})
}

fn assert_limited(reply: Reply) {
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(reply.body, json!({"error": "rate_limited"}));
    assert_eq!(reply.retry_after.as_deref(), Some("60"));
}

fn assert_replay(reply: Reply, original: &Value) {
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, *original);
    assert!(reply.retry_after.is_none());
}

async fn provision(app: &Router) -> String {
    let reply = request(
        app,
        "POST",
        "/v1/admin/reporters",
        Some(ADMIN),
        Some(json!({})),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED);
    reply.body["token"].as_str().unwrap().to_owned()
}

async fn submit(app: &Router, reporter: &str, body: Value) -> Reply {
    request(app, "POST", "/v1/corrections", Some(reporter), Some(body)).await
}

async fn pending(app: &Router) -> BTreeSet<String> {
    let mut cursor = 0;
    let mut ids = BTreeSet::new();
    for _ in 0..33 {
        let page = request(
            app,
            "GET",
            &format!("/v1/admin/corrections?after={cursor}"),
            Some(ADMIN),
            None,
        )
        .await;
        assert_eq!(page.status, StatusCode::OK);
        let suggestions = page.body["suggestions"].as_array().unwrap();
        assert!(suggestions.len() <= 32);
        for suggestion in suggestions {
            assert_eq!(suggestion["status"], "pending");
            assert!(ids.insert(suggestion["correction_id"].as_str().unwrap().to_owned()));
        }
        assert!(ids.len() <= 1024);
        if let Some(next) = page.body["next_after"].as_i64() {
            assert_eq!(suggestions.len(), 32);
            assert!(next > cursor);
            cursor = next;
        } else {
            return ids;
        }
    }
    panic!("Owned global correction queue exceeded its fixture page bound")
}

struct Reply {
    status: StatusCode,
    body: Value,
    retry_after: Option<String>,
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Reply {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = match body {
        Some(body) => {
            request = request.header("content-type", "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let response = bounded(app.clone().oneshot(request.body(body).unwrap()))
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap())
            .unwrap()
            .get_version_num(),
        4
    );
    let status = response.status();
    let retry_after = response
        .headers()
        .get("retry-after")
        .map(|value| value.to_str().unwrap().to_owned());
    let bytes = bounded(to_bytes(response.into_body(), 16_384))
        .await
        .unwrap();
    Reply {
        status,
        body: serde_json::from_slice(&bytes).unwrap(),
        retry_after,
    }
}

async fn bounded<T>(operation: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .expect("Global correction limit operation exceeded its deadline")
}

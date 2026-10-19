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
async fn correction_admission_is_durable_reporter_scoped_and_replays_precede_temporary_limits() {
    tokio::time::timeout(Duration::from_secs(45), admission())
        .await
        .expect("Owned correction admission fixture exceeded 45 seconds");
}

async fn admission() {
    let clock = Clock::new(NOW);
    let fixture = tempfile::tempdir().unwrap();
    let config = || Config {
        state_directory: fixture.path().join("private-correction-limit-state"),
        admin_token: ADMIN.to_owned(),
    };
    let service = bounded(Service::open(config())).await.unwrap();
    let app = router(service.clone());
    let reporter = provision(&app).await;
    let peer = provision(&app).await;
    let first = suggestion();
    let mut invalid = suggestion();
    invalid["left"]["recording_id"] = json!("042");
    assert_eq!(
        submit(&app, &reporter, invalid.clone()).await.status,
        StatusCode::BAD_REQUEST
    );
    let original = submit(&app, &reporter, first.clone()).await;
    assert_eq!(original.status, StatusCode::CREATED);
    let mut conflict = first.clone();
    conflict["left"]["recording_id"] = json!("43");
    for _ in 1..9 {
        assert_eq!(
            submit(&app, &reporter, suggestion()).await.status,
            StatusCode::CREATED
        );
    }
    let left = suggestion();
    let right = suggestion();
    let (left_reply, right_reply) = tokio::join!(
        submit(&app, &reporter, left.clone()),
        submit(&app, &reporter, right.clone())
    );
    let mut statuses = [left_reply.status, right_reply.status];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::CREATED, StatusCode::TOO_MANY_REQUESTS]
    );
    let (accepted, accepted_receipt, blocked, limited) = if left_reply.status == StatusCode::CREATED
    {
        (left, left_reply.body, right, right_reply)
    } else {
        (right, right_reply.body, left, left_reply)
    };
    assert_limited(limited);
    let admitted_replay = submit(&app, &reporter, accepted).await;
    assert_eq!(admitted_replay.status, StatusCode::OK);
    assert_eq!(admitted_replay.body, accepted_receipt);
    assert_limited(submit(&app, &reporter, blocked.clone()).await);
    let replay = submit(&app, &reporter, first.clone()).await;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(replay.body, original.body);
    assert!(replay.retry_after.is_none());
    let mut reversed = first.clone();
    reversed["left"] = first["right"].clone();
    reversed["right"] = first["left"].clone();
    let reverse_replay = submit(&app, &reporter, reversed).await;
    assert_eq!(reverse_replay.status, StatusCode::OK);
    assert_eq!(reverse_replay.body, original.body);
    let conflicting = submit(&app, &reporter, conflict.clone()).await;
    assert_eq!(conflicting.status, StatusCode::CONFLICT);
    assert_eq!(conflicting.body, json!({"error": "submission_conflict"}));
    assert!(conflicting.retry_after.is_none());
    assert_eq!(
        submit(&app, &reporter, invalid.clone()).await.status,
        StatusCode::BAD_REQUEST
    );
    let peer_receipt = submit(&app, &peer, first.clone()).await;
    assert_eq!(peer_receipt.status, StatusCode::CREATED);
    assert_ne!(
        peer_receipt.body["correction_id"],
        original.body["correction_id"]
    );
    assert_eq!(pending(&app).await.len(), 11);
    bounded(service.close()).await;
    drop(app);
    drop(service);

    let reopened = bounded(Service::open(config())).await.unwrap();
    let app = router(reopened.clone());
    assert_limited(submit(&app, &reporter, blocked.clone()).await);
    clock.set(NOW + 59);
    assert_limited(submit(&app, &reporter, blocked.clone()).await);
    assert_eq!(pending(&app).await.len(), 11);
    clock.set(NOW + 60);
    assert_eq!(
        submit(&app, &reporter, blocked).await.status,
        StatusCode::CREATED
    );
    for _ in 1..10 {
        assert_eq!(
            submit(&app, &reporter, suggestion()).await.status,
            StatusCode::CREATED
        );
    }
    clock.set(NOW + 120);
    for _ in 0..10 {
        assert_eq!(
            submit(&app, &reporter, suggestion()).await.status,
            StatusCode::CREATED
        );
    }
    clock.set(NOW + 180);
    for _ in 0..2 {
        assert_eq!(
            submit(&app, &reporter, suggestion()).await.status,
            StatusCode::CREATED
        );
    }
    let pending_full = pending(&app).await;
    assert_eq!(pending_full.len(), 33);
    assert_limited(submit(&app, &reporter, suggestion()).await);
    assert_eq!(pending(&app).await, pending_full);
    let retained_replay = submit(&app, &reporter, first.clone()).await;
    assert_eq!(retained_replay.status, StatusCode::OK);
    assert_eq!(retained_replay.body, original.body);
    assert_eq!(
        submit(&app, &reporter, conflict).await.status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        submit(&app, &reporter, invalid).await.status,
        StatusCode::BAD_REQUEST
    );
    let path = format!(
        "/v1/admin/corrections/{}/review",
        original.body["correction_id"].as_str().unwrap()
    );
    let approved = request(
        &app,
        "POST",
        &path,
        Some(ADMIN),
        Some(json!({
            "request_id": Uuid::new_v4(), "expected_revision": 1, "decision": "approve"
        })),
    )
    .await;
    assert_eq!(approved.status, StatusCode::CREATED);
    assert_eq!(approved.body["revision"], 2);
    assert_eq!(pending(&app).await.len(), 32);
    let newly_accepted = submit(&app, &reporter, suggestion()).await;
    assert_eq!(newly_accepted.status, StatusCode::CREATED);
    assert_eq!(pending(&app).await.len(), 33);
    assert_limited(submit(&app, &reporter, suggestion()).await);
    let original_replay = submit(&app, &reporter, first.clone()).await;
    assert_eq!(original_replay.status, StatusCode::OK);
    assert_eq!(original_replay.body, original.body);
    let peer_replay = submit(&app, &peer, first).await;
    assert_eq!(peer_replay.status, StatusCode::OK);
    assert_eq!(peer_replay.body, peer_receipt.body);
    assert!(
        clock.reads() >= 34,
        "Real SQLite rows did not use the fixture clock"
    );
    bounded(reopened.close()).await;
}

fn suggestion() -> Value {
    json!({"submission_id": Uuid::new_v4(), "left": {"provider": "jamendo", "recording_id": "42"}, "right": {"provider": "youtube", "recording_id": "PrivTone001"}, "relation": "same_recording"})
}

fn assert_limited(reply: Reply) {
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(reply.body, json!({"error": "rate_limited"}));
    assert_eq!(reply.retry_after.as_deref(), Some("60"));
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
    for _ in 0..3 {
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
        if let Some(next) = page.body["next_after"].as_i64() {
            assert_eq!(suggestions.len(), 32);
            assert!(next > cursor);
            cursor = next;
        } else {
            return ids;
        }
    }
    panic!("Owned correction queue exceeded its fixture page bound")
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
        .expect("Correction limit workflow exceeded its deadline")
}

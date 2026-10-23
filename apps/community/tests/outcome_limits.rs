#![cfg(target_os = "linux")]

#[path = "support/sqlite_clock.rs"]
mod sqlite_clock;

use std::{future::Future, time::Duration};

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
async fn durable_rolling_reporter_limit_preserves_replay_conflicts_peers_and_exact_expiry() {
    let clock = Clock::new(NOW);
    let fixture = tempfile::tempdir().unwrap();
    let config = || Config {
        state_directory: fixture.path().join("private-outcome-limit-state"),
        admin_token: ADMIN.to_owned(),
    };
    let service = bounded(Service::open(config())).await.unwrap();
    let app = router(service.clone());
    let reporter = provision(&app).await;
    let peer = provision(&app).await;
    let mut invalid = claim(Uuid::new_v4(), "42");
    invalid["source"]["recording_id"] = json!("042");
    assert_eq!(
        submit(&app, &reporter, invalid.clone()).await.status,
        StatusCode::BAD_REQUEST
    );
    let first = claim(Uuid::new_v4(), "42");
    let first_reply = submit(&app, &reporter, first.clone()).await;
    assert_eq!(first_reply.status, StatusCode::CREATED);
    assert_eq!(first_reply.body["status"], "accepted");
    assert!(first_reply.retry_after.is_none());
    assert_eq!(
        submit(&app, &reporter, first.clone()).await.body,
        first_reply.body
    );
    for _ in 1..300 {
        assert_eq!(
            submit(&app, &reporter, claim(Uuid::new_v4(), "42"))
                .await
                .status,
            StatusCode::CREATED
        );
    }
    let blocked = claim(Uuid::new_v4(), "44");
    assert_limited(submit(&app, &reporter, blocked.clone()).await);
    let replay = submit(&app, &reporter, first.clone()).await;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(replay.body, first_reply.body);
    assert!(replay.retry_after.is_none());
    let mut conflict = first.clone();
    conflict["source"]["recording_id"] = json!("43");
    let rejected = submit(&app, &reporter, conflict).await;
    assert_eq!(rejected.status, StatusCode::CONFLICT);
    assert_eq!(rejected.body, json!({"error": "submission_conflict"}));
    assert!(rejected.retry_after.is_none());
    assert_eq!(
        submit(&app, &reporter, invalid).await.status,
        StatusCode::BAD_REQUEST
    );
    let peer_claim = claim(Uuid::new_v4(), "43");
    let peer_receipt = submit(&app, &peer, peer_claim.clone()).await;
    assert_eq!(peer_receipt.status, StatusCode::CREATED);
    assert_eq!(rank(&app).await, [1, 1, 0]);
    assert!(
        clock.reads() >= 302,
        "SQLite did not use the controlled clock for actual rows"
    );
    bounded(service.close()).await;
    drop(app);
    drop(service);

    let reopened = bounded(Service::open(config())).await.unwrap();
    let app = router(reopened.clone());
    assert_limited(submit(&app, &reporter, blocked.clone()).await);
    let replay = submit(&app, &reporter, first).await;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(replay.body, first_reply.body);
    let peer_replay = submit(&app, &peer, peer_claim).await;
    assert_eq!(peer_replay.status, StatusCode::OK);
    assert_eq!(peer_replay.body, peer_receipt.body);
    clock.set(NOW + 59);
    assert_limited(submit(&app, &reporter, blocked.clone()).await);
    assert_eq!(rank(&app).await, [1, 1, 0]);
    clock.set(NOW + 60);
    assert_eq!(
        submit(&app, &reporter, blocked).await.status,
        StatusCode::CREATED
    );
    assert_eq!(rank(&app).await, [1, 1, 1]);
    for _ in 1..299 {
        assert_eq!(
            submit(&app, &reporter, claim(Uuid::new_v4(), "44"))
                .await
                .status,
            StatusCode::CREATED
        );
    }
    let left = claim(Uuid::new_v4(), "44");
    let right = claim(Uuid::new_v4(), "44");
    let (left_reply, right_reply) = tokio::join!(
        submit(&app, &reporter, left.clone()),
        submit(&app, &reporter, right.clone()),
    );
    let mut statuses = [left_reply.status, right_reply.status];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::CREATED, StatusCode::TOO_MANY_REQUESTS]
    );
    let (accepted, receipt, rejected) = if left_reply.status == StatusCode::CREATED {
        (left, left_reply.body, right)
    } else {
        (right, right_reply.body, left)
    };
    assert_limited(submit(&app, &reporter, rejected).await);
    let replay = submit(&app, &reporter, accepted).await;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(replay.body, receipt);
    assert_eq!(rank(&app).await, [1, 1, 1]);
    bounded(reopened.close()).await;
}

fn claim(submission: Uuid, recording: &str) -> Value {
    json!({
        "submission_id": submission,
        "source": {"provider": "jamendo", "recording_id": recording},
        "outcome": "successful_playback"
    })
}

fn assert_limited(reply: Reply) {
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(reply.body, json!({"error": "rate_limited"}));
    assert_eq!(reply.retry_after.as_deref(), Some("60"));
}

async fn provision(app: &Router) -> String {
    let reply = post(app, "/v1/admin/reporters", Some(ADMIN), json!({})).await;
    assert_eq!(reply.status, StatusCode::CREATED);
    reply.body["token"].as_str().unwrap().to_owned()
}

async fn submit(app: &Router, token: &str, body: Value) -> Reply {
    post(app, "/v1/outcomes", Some(token), body).await
}

async fn rank(app: &Router) -> [u64; 3] {
    let reply = post(
        app,
        "/v1/rankings",
        None,
        json!({"candidates": [
            {"provider": "jamendo", "recording_id": "42"},
            {"provider": "jamendo", "recording_id": "43"},
            {"provider": "jamendo", "recording_id": "44"}
        ]}),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let ranked = reply.body["ranked"].as_array().unwrap();
    assert_eq!(ranked.len(), 3);
    std::array::from_fn(|index| {
        let recording = (42 + index).to_string();
        ranked
            .iter()
            .find(|rank| rank["source"]["recording_id"] == recording)
            .unwrap()["successful_reporters"]
            .as_u64()
            .unwrap()
    })
}

struct Reply {
    status: StatusCode,
    body: Value,
    retry_after: Option<String>,
}

async fn post(app: &Router, path: &str, token: Option<&str>, body: Value) -> Reply {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = bounded(
        app.clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap()),
    )
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
        .expect("Outcome limit workflow exceeded its deadline")
}

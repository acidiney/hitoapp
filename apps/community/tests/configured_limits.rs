use std::{future::Future, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use hito_community::{Config, Error, Limits, Service, router};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const ADMIN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";

#[tokio::test]
async fn an_operator_can_lower_each_limit_and_the_lower_one_answers_429() {
    let fixture = tempfile::tempdir().unwrap();
    let limits = Limits {
        correction_recent_per_minute: 2,
        correction_pending_per_reporter: 32,
        correction_pending_total: 1024,
        outcome_recent_per_minute: 3,
    };
    let service = bounded(Service::open(Config {
        state_directory: fixture.path().join("configured-limit-state"),
        admin_token: ADMIN.to_owned(),
        limits,
    }))
    .await
    .unwrap();
    let app = router(service.clone());
    let reporter = provision(&app).await;
    for recording in ["42", "43", "44"] {
        let reply = post(&app, "/v1/outcomes", Some(&reporter), outcome(recording)).await;
        assert_eq!(reply.0, StatusCode::CREATED);
    }
    assert_limited(post(&app, "/v1/outcomes", Some(&reporter), outcome("45")).await);
    for _ in 0..2 {
        let reply = post(&app, "/v1/corrections", Some(&reporter), suggestion()).await;
        assert_eq!(reply.0, StatusCode::CREATED);
    }
    assert_limited(post(&app, "/v1/corrections", Some(&reporter), suggestion()).await);
    bounded(service.close()).await;
}

#[tokio::test]
async fn a_zero_limit_is_a_configuration_error() {
    for index in 0..4 {
        let fixture = tempfile::tempdir().unwrap();
        let mut limits = Limits::default();
        match index {
            0 => limits.correction_recent_per_minute = 0,
            1 => limits.correction_pending_per_reporter = 0,
            2 => limits.correction_pending_total = 0,
            _ => limits.outcome_recent_per_minute = 0,
        }
        let state = fixture.path().join("zero-limit-state");
        let opened = bounded(Service::open(Config {
            state_directory: state.clone(),
            admin_token: ADMIN.to_owned(),
            limits,
        }))
        .await;
        assert!(matches!(opened, Err(Error::InvalidConfiguration)));
        assert!(!state.try_exists().unwrap());
    }
}

fn outcome(recording: &str) -> Value {
    json!({
        "submission_id": Uuid::new_v4(),
        "source": {"provider": "jamendo", "recording_id": recording},
        "outcome": "successful_playback"
    })
}

fn suggestion() -> Value {
    json!({"submission_id": Uuid::new_v4(), "left": {"provider": "jamendo", "recording_id": "42"}, "right": {"provider": "youtube", "recording_id": "PrivTone001"}, "relation": "same_recording"})
}

fn assert_limited(reply: (StatusCode, Value)) {
    assert_eq!(reply.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(reply.1, json!({"error": "rate_limited"}));
}

async fn provision(app: &Router) -> String {
    let reply = post(app, "/v1/admin/reporters", Some(ADMIN), json!({})).await;
    assert_eq!(reply.0, StatusCode::CREATED);
    reply.1["token"].as_str().unwrap().to_owned()
}

async fn post(app: &Router, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
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
    let status = response.status();
    let bytes = bounded(to_bytes(response.into_body(), 16_384))
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn bounded<T>(operation: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .expect("Configured limit workflow exceeded its deadline")
}

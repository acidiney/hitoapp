use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use hito_community::{Config, Service, router};
use serde_json::{Value, json};
use std::{future::Future, time::Duration};
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";
const SUBMISSION_ID: &str = "37729b20-c721-468a-8473-bc4783b75c9f";

#[tokio::test]
async fn accepted_playback_survives_restart_and_retry_without_inflating_ranking() {
    let directory = tempfile::tempdir().unwrap();
    let config = || Config {
        state_directory: directory.path().join("community"),
        admin_token: ADMIN_TOKEN.to_owned(),
    };
    let service = phase("open", Service::open(config())).await.unwrap();
    let app = router(service.clone());
    let (status, reporter) = phase(
        "provision",
        post(&app, "/v1/admin/reporters", Some(ADMIN_TOKEN), json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let reporter_token = reporter["token"].as_str().unwrap().to_owned();
    let outcome = json!({
        "submission_id": SUBMISSION_ID,
        "source": { "provider": "jamendo", "recording_id": "42" },
        "outcome": "successful_playback"
    });
    let (status, receipt) = phase(
        "submit",
        post(&app, "/v1/outcomes", Some(&reporter_token), outcome.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(receipt["submission_id"], SUBMISSION_ID);
    assert_eq!(receipt["status"], "accepted");
    let receipt_id = receipt["receipt_id"].as_str().unwrap().to_owned();
    assert!(!receipt_id.is_empty());

    phase("first_close", service.close()).await;
    drop(app);
    drop(service);

    let reopened = phase("reopen", Service::open(config())).await.unwrap();
    let app = router(reopened.clone());
    let (status, replay) = phase(
        "replay",
        post(&app, "/v1/outcomes", Some(&reporter_token), outcome),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["receipt_id"], receipt_id);
    assert_eq!(replay["submission_id"], SUBMISSION_ID);

    let (status, ranking) = phase(
        "rank",
        post(
            &app,
            "/v1/rankings",
            None,
            json!({
                "candidates": [
                    { "provider": "youtube", "recording_id": "HitoTone001" },
                    { "provider": "jamendo", "recording_id": "42" }
                ]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        ranking,
        json!({
            "ranked": [
                {
                    "source": { "provider": "jamendo", "recording_id": "42" },
                    "successful_reporters": 1
                },
                {
                    "source": { "provider": "youtube", "recording_id": "HitoTone001" },
                    "successful_reporters": 0
                }
            ]
        })
    );
    phase("final_close", reopened.close()).await;
}

async fn phase<T>(name: &'static str, operation: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .unwrap_or_else(|_| panic!("community_tracer phase={name} exceeded 10 seconds"))
}

async fn post(app: &Router, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65_536).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

use std::{future::Future, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use hito_community::{Config, Service, router};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const ADMIN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";

#[tokio::test]
async fn pending_correction_is_private_durable_and_cannot_change_success_rankings() {
    let fixture = tempfile::tempdir().unwrap();
    let config = || Config {
        state_directory: fixture.path().join("private-correction-submission-state"),
        admin_token: ADMIN.to_owned(),
    };
    let service = bounded(Service::open(config())).await.unwrap();
    let app = router(service.clone());
    let reporter = provision(&app).await;
    let peer = provision(&app).await;
    let peer_outcome = json!({
        "submission_id": Uuid::new_v4(),
        "source": jamendo("42"),
        "outcome": "successful_playback"
    });
    let peer_receipt = post(&app, "/v1/outcomes", Some(&peer), peer_outcome.clone()).await;
    assert_eq!(peer_receipt.0, StatusCode::CREATED);
    let original_ranking = rank(&app).await;
    assert_eq!(original_ranking["ranked"][0]["successful_reporters"], 1);
    assert_eq!(original_ranking["ranked"][1]["successful_reporters"], 0);

    let submission_id = Uuid::new_v4();
    let suggestion = json!({
        "submission_id": submission_id,
        "left": youtube(),
        "right": jamendo("42"),
        "relation": "same_recording"
    });
    let (status, receipt) =
        post(&app, "/v1/corrections", Some(&reporter), suggestion.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    let correction_id = Uuid::parse_str(receipt["correction_id"].as_str().unwrap()).unwrap();
    assert_eq!(correction_id.get_version_num(), 4);
    assert_eq!(
        receipt,
        json!({
            "correction_id": correction_id,
            "submission_id": submission_id,
            "status": "pending"
        })
    );
    for token in [None, Some(reporter.as_str()), Some(peer.as_str())] {
        assert_eq!(
            request(&app, "GET", "/v1/admin/corrections", token, None).await,
            (StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"}))
        );
    }
    let queue = pending(&app).await;
    let entries = queue["suggestions"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    let created_at = entries[0]["created_at"].as_i64().unwrap();
    assert!(created_at > 0);
    assert_eq!(
        queue,
        json!({
            "suggestions": [{
                "correction_id": correction_id,
                "left": jamendo("42"),
                "right": youtube(),
                "relation": "same_recording",
                "status": "pending",
                "revision": 1,
                "created_at": created_at
            }],
            "next_after": null
        })
    );
    assert_eq!(rank(&app).await, original_ranking);
    bounded(service.close()).await;
    drop(app);
    drop(service);

    let reopened = bounded(Service::open(config())).await.unwrap();
    let app = router(reopened.clone());
    assert_eq!(
        post(&app, "/v1/corrections", Some(&reporter), suggestion.clone()).await,
        (StatusCode::OK, receipt.clone())
    );
    let mut reversed = suggestion.clone();
    reversed["left"] = jamendo("42");
    reversed["right"] = youtube();
    assert_eq!(
        post(&app, "/v1/corrections", Some(&reporter), reversed).await,
        (StatusCode::OK, receipt)
    );
    let mut conflicting = suggestion;
    conflicting["right"] = jamendo("43");
    assert_eq!(
        post(&app, "/v1/corrections", Some(&reporter), conflicting).await,
        (
            StatusCode::CONFLICT,
            json!({"error": "submission_conflict"})
        )
    );
    assert_eq!(pending(&app).await, queue);
    assert_eq!(rank(&app).await, original_ranking);
    assert_eq!(
        post(&app, "/v1/outcomes", Some(&peer), peer_outcome).await,
        (StatusCode::OK, peer_receipt.1)
    );
    bounded(reopened.close()).await;
}

fn jamendo(recording: &str) -> Value {
    json!({"provider": "jamendo", "recording_id": recording})
}

fn youtube() -> Value {
    json!({"provider": "youtube", "recording_id": "PrivTone001"})
}

async fn provision(app: &Router) -> String {
    let (status, credential) = post(app, "/v1/admin/reporters", Some(ADMIN), json!({})).await;
    assert_eq!(status, StatusCode::CREATED);
    credential["token"].as_str().unwrap().to_owned()
}

async fn pending(app: &Router) -> Value {
    let (status, queue) = request(app, "GET", "/v1/admin/corrections", Some(ADMIN), None).await;
    assert_eq!(status, StatusCode::OK);
    queue
}

async fn rank(app: &Router) -> Value {
    let (status, ranking) = post(
        app,
        "/v1/rankings",
        None,
        json!({"candidates": [jamendo("42"), youtube()]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    ranking
}

async fn post(app: &Router, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
    request(app, "POST", path, token, Some(body)).await
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
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
    let status = response.status();
    if status != StatusCode::NOT_FOUND {
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap())
                .unwrap()
                .get_version_num(),
            4
        );
    }
    let bytes = bounded(to_bytes(response.into_body(), 16_384))
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

async fn bounded<T>(operation: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .expect("Correction submission exceeded its deadline")
}

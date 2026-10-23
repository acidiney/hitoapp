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
const SUBMISSION: &str = "37729b20-c721-468a-8473-bc4783b75c9f";

#[tokio::test]
async fn administrator_erases_only_the_revoked_reporter_and_restart_preserves_peer_receipts() {
    let fixture = tempfile::tempdir().unwrap();
    let config = || Config {
        state_directory: fixture.path().join("private-revocation-state"),
        admin_token: ADMIN.to_owned(),
    };
    let service = bounded(Service::open(config())).await.unwrap();
    let app = router(service.clone());
    let revoked = provision(&app).await;
    let peer = provision(&app).await;
    let revoked_token = revoked["token"].as_str().unwrap();
    let peer_token = peer["token"].as_str().unwrap();
    let submission = outcome(SUBMISSION, "jamendo", "42");
    assert_eq!(
        post(
            &app,
            "/v1/outcomes",
            Some(revoked_token),
            submission.clone()
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, peer_receipt) =
        post(&app, "/v1/outcomes", Some(peer_token), submission.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        post(
            &app,
            "/v1/outcomes",
            Some(revoked_token),
            outcome(
                "93b08e50-12a8-4328-8cf7-f477c9c47c63",
                "youtube",
                "PrivTone001"
            )
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(counts(&app).await, [2, 1]);

    let revoked_path = format!(
        "/v1/admin/reporters/{}",
        revoked["reporter_id"].as_str().unwrap()
    );
    let (status, body) = request(&app, "DELETE", &revoked_path, Some(ADMIN), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    assert_eq!(counts(&app).await, [1, 0]);

    let peer_path = format!(
        "/v1/admin/reporters/{}",
        peer["reporter_id"].as_str().unwrap()
    );
    for token in [
        None,
        Some(peer_token),
        Some(revoked_token),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    ] {
        let (status, rejection) = request(&app, "DELETE", &peer_path, token, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(rejection, json!({"error": "unauthorized"}));
    }
    let (status, rejection) = request(
        &app,
        "DELETE",
        "/v1/admin/reporters/private-malformed-uuid",
        Some(ADMIN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(rejection, json!({"error": "invalid_input"}));
    for path in [
        revoked_path,
        format!("/v1/admin/reporters/{}", Uuid::new_v4()),
    ] {
        let (status, body) = request(&app, "DELETE", &path, Some(ADMIN), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(body, Value::Null);
    }
    assert_eq!(
        post(
            &app,
            "/v1/outcomes",
            Some(revoked_token),
            submission.clone()
        )
        .await,
        (StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"}))
    );
    assert_eq!(
        post(&app, "/v1/outcomes", Some(peer_token), submission.clone()).await,
        (StatusCode::OK, peer_receipt.clone())
    );
    assert_eq!(
        post(
            &app,
            "/v1/outcomes",
            Some(peer_token),
            outcome(SUBMISSION, "jamendo", "43")
        )
        .await,
        (
            StatusCode::CONFLICT,
            json!({"error": "submission_conflict"})
        )
    );
    assert_eq!(counts(&app).await, [1, 0]);
    bounded(service.close()).await;
    drop(app);
    drop(service);

    let reopened = bounded(Service::open(config())).await.unwrap();
    let app = router(reopened.clone());
    assert_eq!(
        post(
            &app,
            "/v1/outcomes",
            Some(revoked_token),
            submission.clone()
        )
        .await,
        (StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"}))
    );
    assert_eq!(
        post(&app, "/v1/outcomes", Some(peer_token), submission).await,
        (StatusCode::OK, peer_receipt)
    );
    assert_eq!(counts(&app).await, [1, 0]);
    bounded(reopened.close()).await;
}

fn outcome(submission: &str, provider: &str, recording: &str) -> Value {
    json!({
        "submission_id": submission,
        "source": {"provider": provider, "recording_id": recording},
        "outcome": "successful_playback"
    })
}

async fn provision(app: &Router) -> Value {
    let (status, credential) = post(app, "/v1/admin/reporters", Some(ADMIN), json!({})).await;
    assert_eq!(status, StatusCode::CREATED);
    Uuid::parse_str(credential["reporter_id"].as_str().unwrap()).unwrap();
    credential
}

async fn counts(app: &Router) -> [u64; 2] {
    let (status, ranking) = post(
        app,
        "/v1/rankings",
        None,
        json!({"candidates": [
            {"provider": "jamendo", "recording_id": "42"},
            {"provider": "youtube", "recording_id": "PrivTone001"}
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ranked = ranking["ranked"].as_array().unwrap();
    assert_eq!(ranked.len(), 2);
    assert_eq!(
        ranked[0]["source"],
        json!({"provider": "jamendo", "recording_id": "42"})
    );
    assert_eq!(
        ranked[1]["source"],
        json!({"provider": "youtube", "recording_id": "PrivTone001"})
    );
    [
        ranked[0]["successful_reporters"].as_u64().unwrap(),
        ranked[1]["successful_reporters"].as_u64().unwrap(),
    ]
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
        .expect("Community revocation workflow exceeded its deadline")
}

use std::{future::Future, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use hito_community::{Config, Service, router};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use tower::ServiceExt;
use uuid::Uuid;

const ADMIN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";

#[tokio::test]
async fn maintainer_decisions_advance_revisions_while_replays_remain_historical_acknowledgements() {
    let fixture = tempfile::tempdir().unwrap();
    let config = || Config {
        state_directory: fixture.path().join("private-correction-review-state"),
        admin_token: ADMIN.to_owned(),
    };
    let service = bounded(Service::open(config())).await.unwrap();
    let app = router(service.clone());
    let reporter = provision(&app).await;
    let peer = provision(&app).await;
    let owner_suggestion = suggestion("42");
    let (status, receipt) = post(
        &app,
        "/v1/corrections",
        Some(&reporter),
        owner_suggestion.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, peer_receipt) = post(&app, "/v1/corrections", Some(&peer), suggestion("43")).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = receipt["correction_id"].as_str().unwrap();
    let path = format!("/v1/admin/corrections/{id}/review");
    let approval = review(1, "approve");
    let (status, approved) = post(&app, &path, Some(ADMIN), approval.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        approved,
        json!({
            "request_id": approval["request_id"],
            "correction_id": id,
            "revision": 2,
            "decision": "approve",
            "status": "approved",
            "validation_method": "maintainer"
        })
    );
    for token in [None, Some(reporter.as_str()), Some(peer.as_str())] {
        assert_eq!(
            post(&app, &path, token, approval.clone()).await,
            (StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"}))
        );
    }
    assert_eq!(
        post(&app, &path, Some(ADMIN), review(1, "approve")).await,
        (StatusCode::CONFLICT, json!({"error": "revision_conflict"}))
    );
    let mut conflict = approval.clone();
    conflict["expected_revision"] = json!(2);
    assert_eq!(
        post(&app, &path, Some(ADMIN), conflict).await,
        (
            StatusCode::CONFLICT,
            json!({"error": "submission_conflict"})
        )
    );
    assert_eq!(
        post(&app, &path, Some(ADMIN), approval.clone()).await,
        (StatusCode::OK, approved.clone())
    );
    let approved_queue = list(&app, "approved").await;
    assert_eq!(approved_queue["suggestions"].as_array().unwrap().len(), 1);
    assert_eq!(approved_queue["suggestions"][0]["correction_id"], id);
    assert_eq!(approved_queue["suggestions"][0]["revision"], 2);

    let repeated_decision = review(2, "approve");
    let (status, repeated) = post(&app, &path, Some(ADMIN), repeated_decision.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        repeated,
        json!({
            "request_id": repeated_decision["request_id"],
            "correction_id": id,
            "revision": 3,
            "decision": "approve",
            "status": "approved",
            "validation_method": "maintainer"
        })
    );
    let rejection = review(3, "reject");
    let (status, rejected) = post(&app, &path, Some(ADMIN), rejection.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        rejected,
        json!({
            "request_id": rejection["request_id"],
            "correction_id": id,
            "revision": 4,
            "decision": "reject",
            "status": "rejected",
            "validation_method": "maintainer"
        })
    );
    assert_eq!(
        post(&app, &path, Some(ADMIN), approval.clone()).await,
        (StatusCode::OK, approved.clone())
    );
    assert!(
        list(&app, "approved").await["suggestions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let rejected_queue = list(&app, "rejected").await;
    assert_eq!(rejected_queue["suggestions"].as_array().unwrap().len(), 1);
    assert_eq!(rejected_queue["suggestions"][0]["correction_id"], id);
    assert_eq!(rejected_queue["suggestions"][0]["revision"], 4);
    let pending = list(&app, "pending").await;
    assert_eq!(pending["suggestions"].as_array().unwrap().len(), 1);
    assert_eq!(
        pending["suggestions"][0]["correction_id"],
        peer_receipt["correction_id"]
    );
    assert_eq!(pending["suggestions"][0]["revision"], 1);
    assert_eq!(
        post(
            &app,
            "/v1/corrections",
            Some(&reporter),
            owner_suggestion.clone()
        )
        .await,
        (StatusCode::OK, receipt.clone())
    );
    bounded(service.close()).await;
    drop(app);
    drop(service);

    let reopened = bounded(Service::open(config())).await.unwrap();
    let app = router(reopened.clone());
    for (request, acknowledgement) in [
        (approval, approved),
        (repeated_decision, repeated),
        (rejection, rejected),
    ] {
        assert_eq!(
            post(&app, &path, Some(ADMIN), request).await,
            (StatusCode::OK, acknowledgement)
        );
    }
    assert_eq!(list(&app, "rejected").await, rejected_queue);
    assert_eq!(list(&app, "pending").await, pending);
    assert_eq!(
        post(&app, "/v1/corrections", Some(&reporter), owner_suggestion).await,
        (StatusCode::OK, receipt)
    );
    bounded(reopened.close()).await;
}

#[tokio::test]
async fn review_revision_boundary_is_exact_in_runtime_storage_and_generated_contract() {
    const MAX_EXPECTED: i64 = 9_007_199_254_740_990;
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("private-review-revision-boundary");
    let config = || Config {
        state_directory: directory.clone(),
        admin_token: ADMIN.to_owned(),
    };
    let service = bounded(Service::open(config())).await.unwrap();
    let app = router(service.clone());
    let reporter = provision(&app).await;
    let (status, receipt) = post(&app, "/v1/corrections", Some(&reporter), suggestion("42")).await;
    assert_eq!(status, StatusCode::CREATED);
    let correction_id = receipt["correction_id"].as_str().unwrap();
    bounded(service.close()).await;
    drop(app);
    drop(service);

    let options = SqliteConnectOptions::new()
        .filename(directory.join("community.sqlite"))
        .create_if_missing(false)
        .foreign_keys(true)
        .disable_statement_logging();
    let mut connection = bounded(SqliteConnection::connect_with(&options))
        .await
        .unwrap();
    let seeded =
        sqlx::query("UPDATE correction_suggestions SET revision = ? WHERE correction_id = ?")
            .bind(MAX_EXPECTED)
            .bind(correction_id)
            .execute(&mut connection)
            .await
            .unwrap();
    assert_eq!(seeded.rows_affected(), 1);
    connection.close().await.unwrap();

    let reopened = bounded(Service::open(config())).await.unwrap();
    let app = router(reopened.clone());
    let path = format!("/v1/admin/corrections/{correction_id}/review");
    for invalid in [0, -1, MAX_EXPECTED + 1, i64::MAX] {
        assert_eq!(
            post(&app, &path, Some(ADMIN), review(invalid, "approve")).await,
            (StatusCode::BAD_REQUEST, json!({"error": "invalid_input"}))
        );
    }
    let review_request = review(MAX_EXPECTED, "approve");
    let (status, acknowledged) = post(&app, &path, Some(ADMIN), review_request.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(acknowledged["revision"].as_i64(), Some(MAX_EXPECTED + 1));
    assert_eq!(
        list(&app, "approved").await["suggestions"][0]["revision"].as_i64(),
        Some(MAX_EXPECTED + 1)
    );
    assert_eq!(
        post(&app, &path, Some(ADMIN), review(MAX_EXPECTED + 1, "reject")).await,
        (StatusCode::BAD_REQUEST, json!({"error": "invalid_input"}))
    );
    assert_eq!(
        post(&app, &path, Some(ADMIN), review_request).await,
        (StatusCode::OK, acknowledged)
    );
    let (status, contract) = request(&app, "GET", "/v1/openapi.json", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let schemas = &contract["components"]["schemas"];
    assert_eq!(
        schemas["ReviewCorrection"]["properties"]["expected_revision"]["maximum"].as_f64(),
        Some(MAX_EXPECTED as f64)
    );
    for schema in ["ReviewReceipt", "CorrectionSummary"] {
        assert_eq!(
            schemas[schema]["properties"]["revision"]["maximum"].as_f64(),
            Some((MAX_EXPECTED + 1) as f64)
        );
    }
    bounded(reopened.close()).await;
}

fn suggestion(recording: &str) -> Value {
    json!({
        "submission_id": Uuid::new_v4(),
        "left": {"provider": "jamendo", "recording_id": recording},
        "right": {"provider": "youtube", "recording_id": "PrivTone001"},
        "relation": "same_recording"
    })
}

fn review(revision: i64, decision: &str) -> Value {
    json!({"request_id": Uuid::new_v4(), "expected_revision": revision, "decision": decision})
}

async fn provision(app: &Router) -> String {
    let (status, credential) = post(app, "/v1/admin/reporters", Some(ADMIN), json!({})).await;
    assert_eq!(status, StatusCode::CREATED);
    credential["token"].as_str().unwrap().to_owned()
}

async fn list(app: &Router, status: &str) -> Value {
    let path = format!("/v1/admin/corrections?status={status}");
    let (status, page) = request(app, "GET", &path, Some(ADMIN), None).await;
    assert_eq!(status, StatusCode::OK);
    page
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
        .expect("Correction review exceeded its deadline")
}

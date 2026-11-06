use super::support::{call, indexed_app};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn remembered_recordings_resolve_in_order_and_unknown_or_unreadable_ones_are_dropped() {
    let (_dir, app, admin, recording) = indexed_app().await;
    let unknown = Uuid::new_v4().to_string();
    let (status, found) = call(
        &app,
        "POST",
        "/api/v1/recordings/lookup",
        json!({"recording_ids": [recording, unknown, recording]}),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let found = found.as_array().unwrap();
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|item| item["id"] == recording));
    assert_eq!(found[0]["available"], true);
    assert_eq!(found[0]["liked"], false);

    let (status, empty) = call(
        &app,
        "POST",
        "/api/v1/recordings/lookup",
        json!({"recording_ids": []}),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty, json!([]));

    call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"bob","password":"bob private test password"}),
        Some(&admin),
    )
    .await;
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"bob","password":"bob private test password"}),
        None,
    )
    .await;
    let bob = session["token"].as_str().unwrap();
    let (status, hidden) = call(
        &app,
        "POST",
        "/api/v1/recordings/lookup",
        json!({"recording_ids": [recording]}),
        Some(bob),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hidden, json!([]));
}

#[tokio::test]
async fn recording_lookup_needs_a_session_and_a_bounded_valid_request() {
    let (_dir, app, admin, recording) = indexed_app().await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/recordings/lookup",
        json!({"recording_ids": [recording]}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for body in [
        json!({"recording_ids": ["not-an-id"]}),
        json!({"recording_ids": vec![recording.clone(); 501]}),
        json!({"recording_ids": [], "extra": 1}),
        Value::Null,
    ] {
        let (status, _) = call(
            &app,
            "POST",
            "/api/v1/recordings/lookup",
            body,
            Some(&admin),
        )
        .await;
        assert!(
            status.is_client_error(),
            "an invalid lookup must be refused, got {status}"
        );
    }
}

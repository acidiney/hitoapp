use super::support::{call, indexed_app};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

const PREFERENCES: &str = "/api/v1/history/preferences";

async fn guest(app: &axum::Router, admin: &str) -> String {
    let credentials = json!({"username": "guest", "password": "history guest password"});
    let (status, user) = call(
        app,
        "POST",
        "/api/v1/users",
        credentials.clone(),
        Some(admin),
    )
    .await;
    assert!(status.is_success(), "{status}");
    let (_, libraries) = call(app, "GET", "/api/v1/libraries", Value::Null, Some(admin)).await;
    let (status, _) = call(
        app,
        "PUT",
        &format!(
            "/api/v1/libraries/{}/members/{}",
            libraries[0]["id"].as_str().unwrap(),
            user["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(admin),
    )
    .await;
    assert!(status.is_success(), "{status}");
    let (_, session) = call(app, "POST", "/api/v1/login", credentials, None).await;
    session["token"].as_str().unwrap().to_owned()
}

async fn play(app: &axum::Router, token: &str, recording: &str) {
    let batch = json!({"device_id": "history-test", "operations": [{
        "id": uuid::Uuid::new_v4(),
        "action": {"kind": "progress_set", "recording_id": recording, "position_ms": 900, "speed": 1.0}
    }]});
    let (status, _) = call(app, "POST", "/api/v1/sync", batch, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
}

async fn history_len(app: &axum::Router, token: &str) -> usize {
    let (status, activity) = call(app, "GET", "/api/v1/activity", Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    activity["history"].as_array().unwrap().len()
}

#[tokio::test]
async fn history_settings_need_a_session() {
    let (_dir, app, _token, _recording) = indexed_app().await;
    for (method, path, body) in [
        ("GET", PREFERENCES, Value::Null),
        ("PUT", PREFERENCES, json!({"enabled": false})),
        ("DELETE", "/api/v1/history", Value::Null),
    ] {
        let (status, _) = call(&app, method, path, body, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
    }
}

#[tokio::test]
async fn pausing_and_deleting_history_only_touch_the_callers_account() {
    let (_dir, app, token, recording) = indexed_app().await;
    let other = guest(&app, &token).await;
    assert_eq!(
        call(&app, "GET", PREFERENCES, Value::Null, Some(&token)).await,
        (StatusCode::OK, json!({"enabled": true}))
    );
    play(&app, &token, &recording).await;
    play(&app, &other, &recording).await;
    assert_eq!(history_len(&app, &token).await, 1);

    assert_eq!(
        call(
            &app,
            "PUT",
            PREFERENCES,
            json!({"enabled": false}),
            Some(&token)
        )
        .await,
        (StatusCode::OK, json!({"enabled": false}))
    );
    assert_eq!(
        call(&app, "GET", PREFERENCES, Value::Null, Some(&other))
            .await
            .1,
        json!({"enabled": true})
    );
    let (status, _) = call(
        &app,
        "PUT",
        PREFERENCES,
        json!({"enabled": "no"}),
        Some(&token),
    )
    .await;
    assert!(status.is_client_error());
    let (status, _) = call(
        &app,
        "PUT",
        PREFERENCES,
        json!({"enabled": true, "extra": 1}),
        Some(&token),
    )
    .await;
    assert!(status.is_client_error());

    assert_eq!(
        call(&app, "DELETE", "/api/v1/history", Value::Null, Some(&token)).await,
        (StatusCode::OK, json!({"removed": 1}))
    );
    assert_eq!(history_len(&app, &token).await, 0);
    assert_eq!(history_len(&app, &other).await, 1);
    play(&app, &token, &recording).await;
    assert_eq!(history_len(&app, &token).await, 0);
    let (_, saved) = call(&app, "GET", "/api/v1/recordings", Value::Null, Some(&token)).await;
    assert!(saved["items"].as_array().unwrap().len() == 1);
    let (status, _) = call(&app, "DELETE", "/api/v1/history", Value::Null, Some(&other)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history_len(&app, &other).await, 0);
}

#[tokio::test]
async fn history_setting_responses_are_never_cached() {
    let (_dir, app, token, _recording) = indexed_app().await;
    for (method, path, body) in [
        ("GET", PREFERENCES, String::new()),
        ("PUT", PREFERENCES, json!({"enabled": true}).to_string()),
        ("DELETE", "/api/v1/history", String::new()),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .header("x-hito-request", "1")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{method} {path}");
        assert_eq!(
            response.headers()["cache-control"],
            "no-store",
            "{method} {path}"
        );
    }
}

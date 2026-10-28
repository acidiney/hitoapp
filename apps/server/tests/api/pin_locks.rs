use super::support::{admin, app, call};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use tower::ServiceExt;

const PASSWORD: &str = "administrator test password";

async fn set_pin(app: &Router, token: &str, pin: &str, idle: u32) -> (StatusCode, Value) {
    call(
        app,
        "PUT",
        "/api/v1/account/pin",
        json!({"password": PASSWORD, "pin": pin, "idle_minutes": idle}),
        Some(token),
    )
    .await
}

async fn status(app: &Router, token: &str, method: &str, path: &str, body: Value) -> StatusCode {
    call(app, method, path, body, Some(token)).await.0
}

async fn page(app: &Router, token: &str, uri: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("x-inertia", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn login(app: &Router, username: &str, password: &str) -> String {
    call(
        app,
        "POST",
        "/api/v1/login",
        json!({"username": username, "password": password}),
        None,
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test]
async fn pin_changes_need_the_password_and_a_valid_pin() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let (state, lock) = call(
        &app,
        "GET",
        "/api/v1/account/lock",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(state, StatusCode::OK);
    assert_eq!(
        lock,
        json!({"pin_enabled": false, "idle_minutes": 0, "locked": false})
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/account/pin",
            json!({"password": "wrong password here", "pin": "1234", "idle_minutes": 0}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        set_pin(&app, &token, "12", 0).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        status(&app, &token, "POST", "/api/v1/account/lock", Value::Null).await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "locking is unavailable without a PIN"
    );
    let (state, lock) = set_pin(&app, &token, "1234", 15).await;
    assert_eq!(state, StatusCode::OK);
    assert_eq!(
        lock,
        json!({"pin_enabled": true, "idle_minutes": 15, "locked": false})
    );
    assert_eq!(set_pin(&app, &token, "654321", 5).await.0, StatusCode::OK);
    assert_eq!(
        status(
            &app,
            &token,
            "POST",
            "/api/v1/account/pin/remove",
            json!({"password": "not the password"})
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (state, lock) = call(
        &app,
        "POST",
        "/api/v1/account/pin/remove",
        json!({"password": PASSWORD}),
        Some(&token),
    )
    .await;
    assert_eq!(state, StatusCode::OK);
    assert_eq!(lock["pin_enabled"], false);
}

#[tokio::test]
async fn a_locked_session_serves_nothing_but_unlock_sign_out_and_lock_state() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    set_pin(&app, &token, "1234", 0).await;
    let (state, lock) = call(
        &app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(state, StatusCode::OK);
    assert_eq!(lock["locked"], true);

    for (method, path) in [
        ("GET", "/api/v1/me"),
        ("GET", "/api/v1/libraries"),
        ("GET", "/api/v1/recordings"),
        ("GET", "/api/v1/users"),
        ("GET", "/api/v1/playlists"),
        ("PUT", "/api/v1/account/pin"),
        ("POST", "/api/v1/account/pin/remove"),
    ] {
        let (state, body) = call(&app, method, path, json!({}), Some(&token)).await;
        assert_eq!(state, StatusCode::LOCKED, "{path}");
        assert_eq!(body["code"], "locked", "{path}");
    }
    let (_, lock) = call(
        &app,
        "GET",
        "/api/v1/account/lock",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(lock["locked"], true);

    let screen = page(&app, &token, "/library").await;
    assert_eq!(screen["component"], "LockPage");
    let text = screen.to_string();
    assert!(
        !text.contains("playlists") && !text.contains("libraries") && !text.contains("recordings")
    );
    assert_eq!(screen["props"]["username"], "admin");
    assert_eq!(
        page(&app, &token, "/settings").await["component"],
        "LockPage"
    );

    assert_eq!(
        status(&app, &token, "POST", "/api/v1/logout", Value::Null).await,
        StatusCode::NO_CONTENT,
        "signing out is always allowed"
    );
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/account/lock", Value::Null).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn the_correct_pin_restores_the_same_session_and_wrong_pins_are_limited() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    set_pin(&app, &token, "4321", 0).await;
    call(
        &app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&token),
    )
    .await;
    for _ in 0..3 {
        let (state, body) = call(
            &app,
            "POST",
            "/api/v1/account/unlock",
            json!({"pin": "0000"}),
            Some(&token),
        )
        .await;
        assert_eq!(state, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!body.to_string().contains("0000"));
    }
    let (state, lock) = call(
        &app,
        "POST",
        "/api/v1/account/unlock",
        json!({"pin": "4321"}),
        Some(&token),
    )
    .await;
    assert_eq!(
        (state, lock["locked"].as_bool()),
        (StatusCode::OK, Some(false))
    );
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::OK
    );
    assert_eq!(
        page(&app, &token, "/library").await["component"],
        "CatalogPage"
    );

    call(
        &app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&token),
    )
    .await;
    for _ in 0..4 {
        call(
            &app,
            "POST",
            "/api/v1/account/unlock",
            json!({"pin": "0000"}),
            Some(&token),
        )
        .await;
    }
    assert_eq!(
        status(
            &app,
            &token,
            "POST",
            "/api/v1/account/unlock",
            json!({"pin": "0000"})
        )
        .await,
        StatusCode::UNAUTHORIZED,
        "the fifth wrong PIN ends the session"
    );
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, "admin", PASSWORD).await.len(),
        64,
        "the password still signs in"
    );
}

#[tokio::test]
async fn locks_do_not_leak_to_other_sessions_or_accounts() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let second_session = login(&app, "admin", PASSWORD).await;
    call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username": "bob", "password": "another long password"}),
        Some(&token),
    )
    .await;
    let bob = login(&app, "bob", "another long password").await;
    set_pin(&app, &token, "1234", 0).await;
    call(
        &app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::LOCKED
    );
    assert_eq!(
        status(&app, &second_session, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, &bob, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, &bob, "POST", "/api/v1/account/lock", Value::Null).await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "another account has no PIN"
    );
}

#[tokio::test]
async fn idle_sessions_lock_when_the_account_chose_an_idle_period() {
    let (dir, app) = app().await;
    let token = admin(&app).await;
    set_pin(&app, &token, "1234", 5).await;
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::OK
    );
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(dir.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE session_locks SET active_at = active_at - 600")
        .execute(&mut connection)
        .await
        .unwrap();
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::LOCKED
    );
    assert_eq!(
        page(&app, &token, "/library").await["component"],
        "LockPage"
    );
}

async fn age(dir: &std::path::Path, seconds: i64) {
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(dir.join("hito.db")))
            .await
            .unwrap();
    sqlx::query("UPDATE session_locks SET active_at = active_at - ?")
        .bind(seconds)
        .execute(&mut connection)
        .await
        .unwrap();
}

#[tokio::test]
async fn background_requests_do_not_defer_the_idle_lock_but_the_activity_signal_does() {
    let (dir, app) = app().await;
    let token = admin(&app).await;
    set_pin(&app, &token, "1234", 5).await;
    age(dir.path(), 3 * 60).await;
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::OK
    );
    age(dir.path(), 3 * 60).await;
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::LOCKED
    );
    call(
        &app,
        "POST",
        "/api/v1/account/unlock",
        json!({"pin": "1234"}),
        Some(&token),
    )
    .await;

    age(dir.path(), 4 * 60).await;
    let (state, lock) = call(
        &app,
        "POST",
        "/api/v1/account/activity",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(
        (state, lock["locked"].as_bool()),
        (StatusCode::OK, Some(false))
    );
    age(dir.path(), 4 * 60).await;
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::OK
    );
    age(dir.path(), 2 * 60).await;
    assert_eq!(
        status(
            &app,
            &token,
            "POST",
            "/api/v1/account/activity",
            Value::Null
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::LOCKED,
        "activity cannot revive an expired session"
    );
}

#[tokio::test]
async fn the_lock_screen_clears_cached_page_history() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    set_pin(&app, &token, "1234", 0).await;
    assert_eq!(page(&app, &token, "/library").await["clearHistory"], false);
    call(
        &app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(page(&app, &token, "/library").await["clearHistory"], true);
}

#[tokio::test]
async fn pin_routes_are_throttled_per_address() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let mut statuses = Vec::new();
    for _ in 0..12 {
        statuses.push(
            call(
                &app,
                "POST",
                "/api/v1/account/pin/remove",
                json!({"password": "not the password"}),
                Some(&token),
            )
            .await
            .0,
        );
    }
    assert!(
        statuses[..10]
            .iter()
            .all(|s| *s == StatusCode::UNPROCESSABLE_ENTITY)
    );
    assert!(
        statuses[10..]
            .iter()
            .all(|s| *s == StatusCode::TOO_MANY_REQUESTS)
    );
    assert_eq!(
        status(
            &app,
            &token,
            "PUT",
            "/api/v1/account/pin",
            json!({"password":"x","pin":"1234","idle_minutes":0})
        )
        .await,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        status(
            &app,
            &token,
            "POST",
            "/api/v1/account/unlock",
            json!({"pin":"1234"})
        )
        .await,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        status(&app, &token, "GET", "/api/v1/me", Value::Null).await,
        StatusCode::OK
    );
}

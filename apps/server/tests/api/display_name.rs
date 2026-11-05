use super::support::{admin, app, call};
use axum::http::StatusCode;
use serde_json::{Value, json};

const PATH: &str = "/api/v1/account/display-name";
const PASSWORD: &str = "a long display name password";

async fn guest(app: &axum::Router, admin: &str) -> String {
    let credentials = json!({"username": "guest", "password": PASSWORD});
    let (status, _) = call(
        app,
        "POST",
        "/api/v1/users",
        credentials.clone(),
        Some(admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, session) = call(app, "POST", "/api/v1/login", credentials, None).await;
    session["token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn setting_the_name_needs_a_session() {
    let (_dir, app) = app().await;
    admin(&app).await;
    assert_eq!(
        call(&app, "PUT", PATH, json!({"display_name": "Ana"}), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn me_and_the_session_user_carry_the_display_name() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let (_, me) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&token)).await;
    assert_eq!(me["display_name"], Value::Null);
    let (status, updated) = call(
        &app,
        "PUT",
        PATH,
        json!({"display_name": "  Ana Souza 🦉  "}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["display_name"], "Ana Souza 🦉");
    assert_eq!(updated["username"], "admin");
    let (_, me) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&token)).await;
    assert_eq!(me["display_name"], "Ana Souza 🦉");
    assert_eq!(me["username"], "admin");
    let (_, again) = call(
        &app,
        "PUT",
        PATH,
        json!({"display_name": null}),
        Some(&token),
    )
    .await;
    assert_eq!(again["display_name"], Value::Null);
}

#[tokio::test]
async fn invalid_names_are_rejected_and_leave_the_name_unchanged() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    call(
        &app,
        "PUT",
        PATH,
        json!({"display_name": "Ana"}),
        Some(&token),
    )
    .await;
    for body in [
        json!({"display_name": "a".repeat(65)}),
        json!({"display_name": "line\nbreak"}),
        json!({"display_name": "Ana\u{202e}"}),
        json!({"display_name": "Ana\u{200b}Souza"}),
    ] {
        assert_eq!(
            call(&app, "PUT", PATH, body, Some(&token)).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    for body in [
        json!({"display_name": 5}),
        json!({"name": "Ana"}),
        json!({"display_name": "Ana", "admin": true}),
    ] {
        let status = call(&app, "PUT", PATH, body, Some(&token)).await.0;
        assert!(
            status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST,
            "{status}"
        );
    }
    let (_, me) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&token)).await;
    assert_eq!(me["display_name"], "Ana");
}

#[tokio::test]
async fn administrators_list_names_but_cannot_change_another_account() {
    let (_dir, app) = app().await;
    let admin_token = admin(&app).await;
    let guest_token = guest(&app, &admin_token).await;
    call(
        &app,
        "PUT",
        PATH,
        json!({"display_name": "Guest Person"}),
        Some(&guest_token),
    )
    .await;
    let (status, accounts) = call(
        &app,
        "GET",
        "/api/v1/users",
        Value::Null,
        Some(&admin_token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let guest = accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|account| account["username"] == "guest")
        .unwrap();
    assert_eq!(guest["display_name"], "Guest Person");
    let admin_row = accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|account| account["username"] == "admin")
        .unwrap();
    assert_eq!(admin_row["display_name"], Value::Null);
    call(
        &app,
        "PUT",
        PATH,
        json!({"display_name": "Boss"}),
        Some(&admin_token),
    )
    .await;
    let (_, me) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&guest_token)).await;
    assert_eq!(me["display_name"], "Guest Person");
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/users",
            Value::Null,
            Some(&guest_token)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_locked_session_cannot_change_the_name() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let (status, _) = call(
        &app,
        "PUT",
        "/api/v1/account/pin",
        json!({"password": "administrator test password", "pin": "1234", "idle_minutes": 0}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    call(
        &app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&token),
    )
    .await;
    let (status, _) = call(
        &app,
        "PUT",
        PATH,
        json!({"display_name": "Nope"}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::LOCKED);
}

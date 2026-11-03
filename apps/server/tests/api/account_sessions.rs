use super::support::{admin, app, call};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const PASSWORD: &str = "administrator test password";
const NEXT: &str = "a brand new long password";

async fn login(app: &Router) -> String {
    call(
        app,
        "POST",
        "/api/v1/login",
        json!({"username": "admin", "password": PASSWORD}),
        None,
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .into()
}

async fn change(app: &Router, token: &str, current: &str, new: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/api/v1/account/password",
        json!({"current_password": current, "new_password": new}),
        Some(token),
    )
    .await
}

async fn me(app: &Router, token: &str) -> StatusCode {
    call(app, "GET", "/api/v1/me", Value::Null, Some(token))
        .await
        .0
}

#[tokio::test]
async fn every_route_needs_a_session() {
    let (_dir, app) = app().await;
    admin(&app).await;
    for (method, path, body) in [
        (
            "POST",
            "/api/v1/account/password",
            json!({"current_password": PASSWORD, "new_password": NEXT}),
        ),
        ("GET", "/api/v1/account/sessions", Value::Null),
        ("DELETE", "/api/v1/account/sessions", Value::Null),
        ("DELETE", "/api/v1/account/sessions/anything", Value::Null),
    ] {
        assert_eq!(
            call(&app, method, path, body, None).await.0,
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn changing_the_password_ends_other_sessions_and_keeps_this_one() {
    let (_dir, app) = app().await;
    let first = admin(&app).await;
    let second = login(&app).await;
    let (status, body) = change(&app, &first, PASSWORD, NEXT).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"revoked": 1}));
    assert_eq!(me(&app, &first).await, StatusCode::OK);
    assert_eq!(me(&app, &second).await, StatusCode::UNAUTHORIZED);
    let old = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username": "admin", "password": PASSWORD}),
        None,
    )
    .await;
    assert_eq!(old.0, StatusCode::UNAUTHORIZED);
    let new = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username": "admin", "password": NEXT}),
        None,
    )
    .await;
    assert_eq!(new.0, StatusCode::OK);
    let rendered = body.to_string();
    for secret in [PASSWORD, NEXT, first.as_str(), second.as_str()] {
        assert!(!rendered.contains(secret));
    }
}

#[tokio::test]
async fn a_short_new_password_is_unprocessable() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let (status, body) = change(&app, &token, PASSWORD, "short").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!body.to_string().contains("short password"));
    assert_eq!(change(&app, &token, PASSWORD, NEXT).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_wrong_current_password_answers_like_a_failed_sign_in_and_is_limited() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let failed_login = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username": "admin", "password": "wrong password here"}),
        None,
    )
    .await;
    let (status, body) = change(&app, &token, "wrong password here", NEXT).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, failed_login.1);
    for _ in 1..5 {
        assert_eq!(
            change(&app, &token, "wrong password here", NEXT).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/account/password")
                .header("content-type", "application/json")
                .header("x-hito-request", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    json!({"current_password": PASSWORD, "new_password": NEXT}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key("retry-after"));
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!text.contains(PASSWORD) && !text.contains(NEXT));
}

#[tokio::test]
async fn the_session_list_has_opaque_ids_and_never_the_secret() {
    let (_dir, app) = app().await;
    let first = admin(&app).await;
    let second = login(&app).await;
    let (status, list) = call(
        &app,
        "GET",
        "/api/v1/account/sessions",
        Value::Null,
        Some(&second),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let sessions = list.as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(
        sessions
            .iter()
            .filter(|session| session["current"] == true)
            .count(),
        1
    );
    let rendered = list.to_string();
    assert!(!rendered.contains(&first) && !rendered.contains(&second));
    for session in sessions {
        assert!(session["id"].as_str().unwrap().len() >= 32);
        assert!(session["created_at"].as_i64().unwrap() > 0);
        assert_eq!(session.as_object().unwrap().len(), 3);
    }
}

#[tokio::test]
async fn sessions_can_be_revoked_one_at_a_time_or_all_others() {
    let (_dir, app) = app().await;
    let first = admin(&app).await;
    let second = login(&app).await;
    let third = login(&app).await;
    let (_, list) = call(
        &app,
        "GET",
        "/api/v1/account/sessions",
        Value::Null,
        Some(&first),
    )
    .await;
    let other = list
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["current"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/v1/account/sessions/{other}"),
        Value::Null,
        Some(&first),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/account/sessions/{other}"),
            Value::Null,
            Some(&first),
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, body) = call(
        &app,
        "DELETE",
        "/api/v1/account/sessions",
        Value::Null,
        Some(&first),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"revoked": 1}));
    assert_eq!(me(&app, &first).await, StatusCode::OK);
    assert_eq!(me(&app, &second).await, StatusCode::UNAUTHORIZED);
    assert_eq!(me(&app, &third).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn another_accounts_session_id_is_not_found() {
    let (_dir, app) = app().await;
    let owner = admin(&app).await;
    call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username": "guest", "password": PASSWORD}),
        Some(&owner),
    )
    .await;
    let guest = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username": "guest", "password": PASSWORD}),
        None,
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, list) = call(
        &app,
        "GET",
        "/api/v1/account/sessions",
        Value::Null,
        Some(&guest),
    )
    .await;
    let id = list[0]["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/account/sessions/{id}"),
            Value::Null,
            Some(&owner),
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(me(&app, &guest).await, StatusCode::OK);
}

#[tokio::test]
async fn revoking_the_current_session_signs_out_and_clears_the_cookie() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let (_, list) = call(
        &app,
        "GET",
        "/api/v1/account/sessions",
        Value::Null,
        Some(&token),
    )
    .await;
    let id = list[0]["id"].as_str().unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/account/sessions/{id}"))
                .header("x-hito-request", "1")
                .header("cookie", format!("hito_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cookie.contains("Max-Age=0") && !cookie.contains(&token));
    assert_eq!(me(&app, &token).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mutations_keep_the_origin_and_request_header_rules() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let cookie = format!("hito_session={token}");
    let body = json!({"current_password": PASSWORD, "new_password": NEXT}).to_string();
    for (method, path, origin, header) in [
        (
            "POST",
            "/api/v1/account/password",
            Some("https://untrusted.example"),
            true,
        ),
        ("POST", "/api/v1/account/password", None, false),
        (
            "DELETE",
            "/api/v1/account/sessions",
            Some("https://untrusted.example"),
            true,
        ),
        ("DELETE", "/api/v1/account/sessions", None, false),
        ("DELETE", "/api/v1/account/sessions/anything", None, false),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("cookie", &cookie);
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        if header {
            request = request.header("x-hito-request", "1");
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.clone())).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
    }
    assert_eq!(me(&app, &token).await, StatusCode::OK);
}

#[tokio::test]
async fn a_locked_session_is_refused_with_423() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    call(
        &app,
        "PUT",
        "/api/v1/account/pin",
        json!({"password": PASSWORD, "pin": "1234", "idle_minutes": 0}),
        Some(&token),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(
        change(&app, &token, PASSWORD, NEXT).await.0,
        StatusCode::LOCKED
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/account/sessions",
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::LOCKED
    );
}

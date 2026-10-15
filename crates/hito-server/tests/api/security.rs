use super::support::{app, call};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn browser_sessions_use_http_only_cookies_and_mutations_reject_cross_origin_requests() {
    let (_dir, app) = app().await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/setup")
                .header("content-type", "application/json")
                .header("x-hito-request", "1")
                .header("origin", "http://localhost:8080")
                .body(Body::from(
                    json!({"username":"admin","password":"administrator test password"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    let cookie = cookie.split(';').next().unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/me")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/logout")
                .header("cookie", cookie)
                .header("x-hito-request", "1")
                .header("origin", "https://untrusted.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/logout")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/logout")
                .header("cookie", cookie)
                .header("x-hito-request", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/me")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn repeated_sign_in_attempts_are_rate_limited() {
    let (_dir, app) = app().await;
    for _ in 0..10 {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/login",
                json!({"username":"unknown","password":"incorrect password"}),
                None
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/login",
            json!({"username":"unknown","password":"incorrect password"}),
            None
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn unfinished_login_bodies_do_not_exhaust_authentication_capacity() {
    let (_dir, app) = app().await;
    let mut pending = Vec::new();
    for _ in 0..4 {
        let app = app.clone();
        pending.push(tokio::spawn(async move {
            let body = Body::from_stream(tokio_stream::pending::<
                Result<axum::body::Bytes, std::io::Error>,
            >());
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/login")
                    .header("content-type", "application/json")
                    .header("x-hito-request", "1")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
        }));
    }
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/login",
            json!({"username":"unknown","password":"incorrect password"}),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    for task in pending {
        assert_eq!(task.await.unwrap(), StatusCode::REQUEST_TIMEOUT);
    }
}

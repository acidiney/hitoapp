use super::support::{app, call};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn configured_lan_origin_preserves_primary_origin_and_mutation_guards() {
    let directory = tempfile::tempdir().unwrap();
    let services =
        super::support::services(directory.path(), &directory.path().join("music")).await;
    let app = hito_server::router_with_config(
        services,
        hito_server::Config {
            origin: "http://localhost:8080".into(),
            additional_origins: vec!["http://192.168.1.50:8080".into()],
            ..Default::default()
        },
    );
    let credentials =
        json!({"username":"admin","password":"administrator test password"}).to_string();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/setup")
                .header("content-type", "application/json")
                .header("x-hito-request", "1")
                .header("origin", "http://localhost:8080")
                .body(Body::from(credentials.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    for (origin, guard, bearer, body, expected) in [
        (
            "http://localhost:8080",
            true,
            false,
            credentials.as_str(),
            StatusCode::OK,
        ),
        (
            "http://192.168.1.50:8080",
            true,
            false,
            credentials.as_str(),
            StatusCode::OK,
        ),
        (
            "http://localhost:8080",
            true,
            false,
            "{",
            StatusCode::BAD_REQUEST,
        ),
        (
            "http://192.168.1.50:8080",
            true,
            false,
            "{",
            StatusCode::BAD_REQUEST,
        ),
        (
            "https://untrusted.example",
            true,
            false,
            "{",
            StatusCode::FORBIDDEN,
        ),
        (
            "https://untrusted.example",
            false,
            true,
            "{",
            StatusCode::FORBIDDEN,
        ),
        (
            "http://192.168.1.50:8080",
            false,
            false,
            "{",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/v1/login")
            .header("content-type", "application/json")
            .header("origin", origin);
        if guard {
            request = request.header("x-hito-request", "1");
        }
        if bearer {
            request = request.header("authorization", "Bearer test-token");
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            expected,
            "origin={origin}, guard={guard}, bearer={bearer}"
        );
    }
}

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

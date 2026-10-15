use super::support::{admin, app, call};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn first_admin_can_sign_in_and_setup_cannot_be_repeated() {
    let (_dir, app) = app().await;
    let account = json!({"username":"alice","password":"a long test password"});
    let (status, auth) = call(&app, "POST", "/api/v1/setup", account.clone(), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let token = auth["token"].as_str().unwrap();
    let (status, me) = call(&app, "GET", "/api/v1/me", Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["username"], "alice");
    assert_eq!(me["admin"], true);
    assert_eq!(
        call(&app, "POST", "/api/v1/setup", account.clone(), None)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&app, "POST", "/api/v1/login", account, None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/me", Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn administrator_creates_accounts_and_controls_shared_library_access() {
    let (_dir, app) = app().await;
    let admin = admin(&app).await;
    let account = json!({"username":"bob","password":"another long password"});
    let (status, bob) = call(&app, "POST", "/api/v1/users", account.clone(), Some(&admin)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, session) = call(&app, "POST", "/api/v1/login", account.clone(), None).await;
    let bob_token = session["token"].as_str().unwrap();
    assert_eq!(
        call(&app, "POST", "/api/v1/users", account, Some(bob_token))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/libraries",
            Value::Null,
            Some(bob_token)
        )
        .await
        .1,
        json!([])
    );
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&admin)).await;
    let library = libraries[0]["id"].as_str().unwrap();
    let access_path = format!(
        "/api/v1/libraries/{library}/members/{}",
        bob["id"].as_str().unwrap()
    );
    assert_eq!(
        call(&app, "PUT", &access_path, Value::Null, Some(&admin))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/libraries",
            Value::Null,
            Some(bob_token)
        )
        .await
        .1[0]["id"],
        library
    );
    assert_eq!(
        call(&app, "DELETE", &access_path, Value::Null, Some(&admin))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/libraries",
            Value::Null,
            Some(bob_token)
        )
        .await
        .1,
        json!([])
    );
}

#[tokio::test]
async fn authenticated_api_and_inertia_requests_reject_the_wrong_account() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    for path in ["/api/v1/me", "/api/v1/playlists", "/library", "/settings"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", format!("Bearer {token}"))
                    .header("x-hito-account", "a-different-account")
                    .header("x-inertia", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

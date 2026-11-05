use super::support::{admin, app, call, services};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::Connection;
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

#[tokio::test]
async fn administrator_deletes_an_account_and_its_sessions_end() {
    let (_dir, app) = app().await;
    let admin = admin(&app).await;
    let account = json!({"username":"bob","password":"another long password"});
    let (_, bob) = call(&app, "POST", "/api/v1/users", account.clone(), Some(&admin)).await;
    let (_, session) = call(&app, "POST", "/api/v1/login", account, None).await;
    let bob_token = session["token"].as_str().unwrap();
    let bob_path = format!("/api/v1/users/{}", bob["id"].as_str().unwrap());
    let (_, me) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&admin)).await;
    let own_path = format!("/api/v1/users/{}", me["id"].as_str().unwrap());

    let name = |username: &str| json!({ "username": username });
    let status = |response: (StatusCode, Value)| response.0;
    assert_eq!(
        status(call(&app, "DELETE", &bob_path, name("bob"), None).await),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status(call(&app, "DELETE", &own_path, name("bob"), Some(bob_token)).await),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            call(
                &app,
                "DELETE",
                "/api/v1/users/unknown",
                name("bob"),
                Some(&admin)
            )
            .await
        ),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status(call(&app, "DELETE", &own_path, name("admin"), Some(&admin)).await),
        StatusCode::CONFLICT
    );
    let (mismatch, body) = call(&app, "DELETE", &bob_path, name("robert"), Some(&admin)).await;
    assert_eq!(mismatch, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("changed"));
    assert_eq!(
        status(
            call(
                &app,
                "DELETE",
                &bob_path,
                json!({"username":"bob","extra":1}),
                Some(&admin)
            )
            .await
        ),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        status(call(&app, "GET", "/api/v1/me", Value::Null, Some(bob_token)).await),
        StatusCode::OK
    );

    assert_eq!(
        status(call(&app, "DELETE", &bob_path, name("bob"), Some(&admin)).await),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(call(&app, "GET", "/api/v1/me", Value::Null, Some(bob_token)).await),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status(call(&app, "DELETE", &bob_path, name("bob"), Some(&admin)).await),
        StatusCode::NOT_FOUND
    );
    let (_, users) = call(&app, "GET", "/api/v1/users", Value::Null, Some(&admin)).await;
    assert_eq!(users.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_administrator_list_carries_only_the_newest_last_active_time() {
    let dir = tempfile::tempdir().unwrap();
    let hito = services(dir.path(), &dir.path().join("music")).await;
    let app = hito_server::router(hito);
    let admin = admin(&app).await;
    call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"quiet","password":"a quiet long password"}),
        Some(&admin),
    )
    .await;
    call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"quiet","password":"a quiet long password"}),
        None,
    )
    .await;
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(dir.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE sessions SET last_seen_at = unixepoch() - 1200
         WHERE user_id = (SELECT id FROM users WHERE username = 'quiet')",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    let (state, users) = call(&app, "GET", "/api/v1/users", Value::Null, Some(&admin)).await;
    assert_eq!(state, StatusCode::OK);
    let quiet = users
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["username"] == "quiet")
        .unwrap();
    let seen = quiet["last_active_at"].as_i64().unwrap();
    assert!((1190..=1260).contains(&(now_seconds() - seen)));
    let mut keys: Vec<&str> = quiet
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        ["admin", "display_name", "id", "last_active_at", "username"]
    );
    let admin_row = users
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["username"] == "admin")
        .unwrap();
    assert!(admin_row["last_active_at"].as_i64().unwrap() >= now_seconds() - 60);
}

#[tokio::test]
async fn members_are_refused_the_list_and_other_views_stay_without_the_field() {
    let (_dir, app) = app().await;
    let admin = admin(&app).await;
    let (_, created) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"member","password":"a member long password"}),
        Some(&admin),
    )
    .await;
    assert!(created.get("last_active_at").is_none());
    let (_, login) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"member","password":"a member long password"}),
        None,
    )
    .await;
    let member = login["token"].as_str().unwrap();
    assert_eq!(
        call(&app, "GET", "/api/v1/users", Value::Null, Some(member))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (_, me) = call(&app, "GET", "/api/v1/me", Value::Null, Some(member)).await;
    assert!(me.get("last_active_at").is_none());
    assert!(login["user"].get("last_active_at").is_none());
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

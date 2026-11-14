use super::support::{call, indexed_app};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::test_support::hang_guard::hang_guard;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const HOME: &str = "/api/v1/home";

async fn get(
    app: &Router,
    path: &str,
    bearer: Option<&str>,
    cookie: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::builder().uri(path).header("x-hito-request", "1");
    if let Some(token) = bearer {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn inertia_home(app: &Router, token: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri("/home")
                .header("x-inertia", "true")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn member(app: &Router, admin: &str) -> (String, String, String) {
    let credentials = json!({"username":"home-member","password":"owned home member password"});
    let (status, user) = call(app, "POST", "/api/v1/users", credentials.clone(), Some(admin)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, libraries) = call(app, "GET", "/api/v1/libraries", Value::Null, Some(admin)).await;
    assert_eq!(status, StatusCode::OK);
    let library_id = libraries[0]["id"].as_str().unwrap();
    let member_id = user["id"].as_str().unwrap();
    let access = format!("/api/v1/libraries/{library_id}/members/{member_id}");
    assert_eq!(call(app, "PUT", &access, Value::Null, Some(admin)).await.0, StatusCode::NO_CONTENT);
    let (status, session) = call(app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    (
        member_id.to_owned(),
        session["token"].as_str().unwrap().to_owned(),
        library_id.to_owned(),
    )
}

#[tokio::test]
async fn home_json_matches_the_authenticated_inertia_home_prop() {
    let (_dir, app, token, recording) = hang_guard(
        "owned indexed Home JSON setup",
        Box::pin(indexed_app()),
    )
    .await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/sync",
        json!({"device_id":"home-json-test","operations":[{
            "id":uuid::Uuid::new_v4(),
            "action":{"kind":"like_set","recording_id":recording,"liked":true}
        }]}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let response = get(&app, HOME, Some(&token), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let (status, json_home) = body(response).await;
    assert_eq!(status, StatusCode::OK);

    let inertia = inertia_home(&app, &token).await;
    assert_eq!(inertia.status(), StatusCode::OK);
    let (_, page) = body(inertia).await;
    assert_eq!(page["component"], "HomePage");
    assert_eq!(json_home, page["props"]["home"]);
    assert_eq!(
        json_home.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(),
        ["has_music", "liked_count", "recent_albums", "recently_added", "unheard"]
    );
    assert_eq!(json_home["has_music"], true);
    assert_eq!(json_home["liked_count"], 1);
    assert!(json_home["recent_albums"].is_array());
    assert!(json_home["recently_added"].is_array());
    assert!(json_home["unheard"].is_array());
    assert!(json_home.get("artists").is_none());
    assert!(json_home.get("props").is_none());
}

#[tokio::test]
async fn home_json_uses_the_authenticated_account_and_its_library_access() {
    let (_dir, app, admin_token, recording) = hang_guard(
        "owned shared Home JSON setup",
        Box::pin(indexed_app()),
    )
    .await;
    let (member_id, member_token, library_id) = member(&app, &admin_token).await;
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/sync",
        json!({"device_id":"member-home-json","operations":[{
            "id":uuid::Uuid::new_v4(),
            "action":{"kind":"like_set","recording_id":recording,"liked":true}
        }]}),
        Some(&member_token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, admin_login) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"admin","password":"administrator test password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let admin_session = admin_login["token"].as_str().unwrap();
    let (_, admin_bearer_home) = body(get(&app, HOME, Some(admin_session), None).await).await;
    let cookie_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/login")
                .header("content-type", "application/json")
                .header("x-hito-request", "1")
                .body(Body::from(json!({"username":"admin","password":"administrator test password"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cookie_response.status(), StatusCode::OK);
    let cookie = cookie_response.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_owned();
    let (status, admin_cookie_home) = body(get(&app, HOME, None, Some(&cookie)).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(admin_cookie_home, admin_bearer_home);
    assert_eq!(admin_cookie_home["liked_count"], 0);

    let (status, member_home) = body(get(&app, HOME, Some(&member_token), Some(&cookie)).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(member_home["liked_count"], 1);
    assert_ne!(member_home, admin_cookie_home);
    let (status, bearer_member_home) = body(get(&app, HOME, Some(&member_token), None).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bearer_member_home, member_home);

    let mismatched = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(HOME)
                .header("authorization", format!("Bearer {member_token}"))
                .header("x-hito-account", "another-account")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mismatched.status(), StatusCode::FORBIDDEN);

    let access = format!("/api/v1/libraries/{library_id}/members/{member_id}");
    assert_eq!(
        call(&app, "DELETE", &access, Value::Null, Some(&admin_token)).await.0,
        StatusCode::NO_CONTENT
    );
    let (status, no_library_home) = body(get(&app, HOME, Some(&member_token), None).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        no_library_home,
        json!({
            "has_music": false,
            "liked_count": 0,
            "recent_albums": [],
            "recently_added": [],
            "unheard": []
        })
    );
}

#[tokio::test]
async fn home_json_requires_an_unlocked_live_session() {
    let (_dir, app, token, _) = hang_guard(
        "owned Home JSON session setup",
        Box::pin(indexed_app()),
    )
    .await;
    assert_eq!(get(&app, HOME, None, None).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        get(&app, HOME, Some("malformed-session-token"), None).await.status(),
        StatusCode::UNAUTHORIZED
    );

    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/account/pin",
            json!({"password":"administrator test password","pin":"1234","idle_minutes":0}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", "/api/v1/account/lock", Value::Null, Some(&token))
            .await
            .0,
        StatusCode::OK
    );
    let locked = body(get(&app, HOME, Some(&token), None).await).await;
    assert_eq!(locked.0, StatusCode::LOCKED);
    assert!(locked.1.get("has_music").is_none());
}

#[tokio::test]
async fn deleting_an_account_invalidates_its_home_session() {
    let (_dir, app, admin_token, _) = hang_guard(
        "owned deleted Home JSON account setup",
        Box::pin(indexed_app()),
    )
    .await;
    let credentials = json!({"username":"deleted-home-member","password":"owned deleted home password"});
    let (status, user) = call(
        &app,
        "POST",
        "/api/v1/users",
        credentials.clone(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, session) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    let token = session["token"].as_str().unwrap();
    assert_eq!(get(&app, HOME, Some(token), None).await.status(), StatusCode::OK);
    let path = format!("/api/v1/users/{}", user["id"].as_str().unwrap());
    assert_eq!(
        call(
            &app,
            "DELETE",
            &path,
            json!({"username":"deleted-home-member"}),
            Some(&admin_token),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, response) = body(get(&app, HOME, Some(token), None).await).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(response.get("has_music").is_none());
}

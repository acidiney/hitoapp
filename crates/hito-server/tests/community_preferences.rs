#[path = "api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use support::call;
use tower::ServiceExt;

const PATH: &str = "/api/v1/community/preferences";

#[tokio::test]
async fn authenticated_accounts_explicitly_save_independent_private_community_consent() {
    let (_directory, app) = support::app().await;
    let admin_token = support::admin(&app).await;
    let (status, admin) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&admin_token)).await;
    assert_eq!(status, StatusCode::OK);
    let admin_id = admin["id"].as_str().unwrap();
    let credentials = json!({
        "username": "consent-listener",
        "password": "owned community listener password"
    });
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/users",
            credentials.clone(),
            Some(&admin_token)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, session) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let listener_token = session["token"].as_str().unwrap();
    let listener_id = session["user"]["id"].as_str().unwrap();
    let disabled =
        json!({"revision": 0, "playback_reporting": false, "metadata_corrections": false});
    let before = [
        collection(&app, &admin_token).await,
        collection(&app, listener_token).await,
    ];

    assert_eq!(get(&app, &admin_token, admin_id).await, disabled);
    assert_eq!(get(&app, listener_token, listener_id).await, disabled);
    for method in ["GET", "PUT"] {
        assert_eq!(
            call(&app, method, PATH, disabled.clone(), None).await.0,
            StatusCode::UNAUTHORIZED
        );
        let (status, denied) = request(
            &app,
            &admin_token,
            listener_id,
            method,
            &disabled.to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(denied.get("playback_reporting").is_none());
        assert!(denied.get("metadata_corrections").is_none());
    }

    let reporting =
        json!({"revision": 1, "playback_reporting": true, "metadata_corrections": false});
    let (status, saved) = request(
        &app,
        &admin_token,
        admin_id,
        "PUT",
        &json!({"revision": 0, "playback_reporting": true, "metadata_corrections": false})
            .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved, reporting);
    assert_eq!(get(&app, listener_token, listener_id).await, disabled);

    let corrections =
        json!({"revision": 1, "playback_reporting": false, "metadata_corrections": true});
    let (status, saved) = request(
        &app,
        listener_token,
        listener_id,
        "PUT",
        &json!({"revision": 0, "playback_reporting": false, "metadata_corrections": true})
            .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved, corrections);
    assert_eq!(get(&app, &admin_token, admin_id).await, reporting);

    for malformed in [
        json!({"revision": 1, "playback_reporting": true, "metadata_corrections": false, "user_id": listener_id}),
        json!({"revision": 1, "playback_reporting": "true", "metadata_corrections": false}),
        json!({"revision": 1, "playback_reporting": true}),
    ] {
        assert_eq!(
            request(&app, &admin_token, admin_id, "PUT", &malformed.to_string())
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(
        request(&app, &admin_token, admin_id, "PUT", "{").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&app, &admin_token, admin_id, "PUT", &disabled.to_string())
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(get(&app, &admin_token, admin_id).await, reporting);
    assert_eq!(get(&app, listener_token, listener_id).await, corrections);
    assert_eq!(collection(&app, &admin_token).await, before[0]);
    assert_eq!(collection(&app, listener_token).await, before[1]);

    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/logout",
            Value::Null,
            Some(listener_token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    for method in ["GET", "PUT"] {
        assert_eq!(
            request(
                &app,
                listener_token,
                listener_id,
                method,
                &corrections.to_string()
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(get(&app, &admin_token, admin_id).await, reporting);
}

async fn get(app: &Router, token: &str, account: &str) -> Value {
    let (status, body) = request(app, token, account, "GET", "").await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn request(
    app: &Router,
    token: &str,
    account: &str,
    method: &str,
    body: &str,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(PATH)
                .header("cookie", format!("hito_session={token}"))
                .header("x-hito-account", account)
                .header("x-hito-request", "1")
                .header("origin", "http://localhost:8080")
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(response.headers().contains_key("x-request-id"));
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn collection(app: &Router, token: &str) -> Value {
    let mut snapshot = serde_json::Map::new();
    for path in [
        "/api/v1/libraries",
        "/api/v1/playlists",
        "/api/v1/activity",
        "/api/v1/sync?cursor=0",
    ] {
        let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
        assert_eq!(status, StatusCode::OK);
        snapshot.insert(path.into(), body);
    }
    Value::Object(snapshot)
}

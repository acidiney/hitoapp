use hito_core::test_support::playback as fixture;
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod http;
use axum::http::StatusCode;
use hito_playback::Player;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn ordinary_servers_report_browser_playback_and_cannot_claim_a_native_player() {
    let (_directory, app) = http::app().await;
    let token = http::admin(&app).await;
    let (status, inspected) =
        http::call(&app, "GET", "/api/v1/player", Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        inspected,
        json!({"native":false, "epoch":null, "state":null})
    );
    assert_eq!(
        http::call(
            &app,
            "POST",
            "/api/v1/player/claim",
            json!({"epoch":Uuid::new_v4()}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::NOT_IMPLEMENTED
    );
}

#[tokio::test]
async fn another_account_cannot_read_or_control_the_old_queue_and_claiming_starts_empty() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let app = hito_server::router_with_player(f.services.clone(), Default::default(), player);
    let credentials =
        json!({"username":"another-listener", "password":"private listener password"});
    assert_eq!(
        http::call(
            &app,
            "POST",
            "/api/v1/users",
            credentials.clone(),
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (_, session) = http::call(&app, "POST", "/api/v1/login", credentials, None).await;
    let another = session["token"].as_str().unwrap();
    let inspected = http::call(&app, "GET", "/api/v1/player", Value::Null, Some(&f.token))
        .await
        .1;
    let attached = http::call(
        &app,
        "POST",
        "/api/v1/player/claim",
        json!({"epoch":inspected["epoch"]}),
        Some(&f.token),
    )
    .await
    .1;
    assert_eq!(
        http::call(
            &app,
            "POST",
            "/api/v1/player/commands",
            json!({
                "id":Uuid::new_v4(), "lease":attached["lease"], "sequence":1, "revision":0,
                "action":{"kind":"replace", "recordings":[f.recording], "start":0}
            }),
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::OK
    );
    let query = json!({"lease":attached["lease"], "revision":1, "offset":0, "limit":10});
    assert_eq!(
        http::call(
            &app,
            "POST",
            "/api/v1/player/queue",
            query.clone(),
            Some(another)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let seen = http::call(&app, "GET", "/api/v1/player", Value::Null, Some(another))
        .await
        .1;
    assert!(seen["state"].is_null());
    let replacement = http::call(
        &app,
        "POST",
        "/api/v1/player/claim",
        json!({"epoch":seen["epoch"]}),
        Some(another),
    )
    .await
    .1;
    assert_eq!(replacement["state"]["queue_len"], 0);
    assert_eq!(replacement["state"]["volume"], 1000);
    assert_eq!(
        http::call(&app, "POST", "/api/v1/player/queue", query, Some(&f.token))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        http::call(
            &app,
            "POST",
            "/api/v1/player/commands",
            json!({
                "id":Uuid::new_v4(), "lease":replacement["lease"], "sequence":1, "revision":0,
                "action":{"kind":"replace", "recordings":[f.recording], "start":0}
            }),
            Some(another)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    owner.close().await.unwrap();
}

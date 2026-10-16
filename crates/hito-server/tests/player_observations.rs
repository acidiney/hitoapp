#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "api/support.rs"]
#[allow(dead_code)]
mod http;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_playback::Player;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;
use uuid::Uuid;

async fn observation(
    app: &Router,
    token: &str,
    lease: &Value,
    kind: &str,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/player/{kind}/events"))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(json!({"lease":lease}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn frame(body: &mut Body) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    let text = std::str::from_utf8(&frame).unwrap();
    serde_json::from_str(text.strip_prefix("data: ").unwrap().trim()).unwrap()
}

#[tokio::test]
async fn state_and_position_streams_are_separate_private_and_close_on_lease_rotation() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let app = hito_server::router_with_player(f.services.clone(), Default::default(), player);
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
    let state = observation(&app, &f.token, &attached["lease"], "state").await;
    assert_eq!(state.status(), StatusCode::OK);
    assert_eq!(state.headers()["cache-control"], "private, no-store");
    assert_eq!(state.headers()["content-type"], "text/event-stream");
    let mut state = state.into_body();
    let position = observation(&app, &f.token, &attached["lease"], "position").await;
    assert_eq!(position.status(), StatusCode::OK);
    let mut position = position.into_body();
    assert_eq!(frame(&mut state).await["queue_len"], 0);
    assert!(frame(&mut position).await["entry"].is_null());
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
    let next = frame(&mut state).await;
    assert_eq!(next["revision"], 1);
    assert!(next.get("position_ms").is_none());
    assert_eq!(frame(&mut position).await["entry"], next["current"]["id"]);
    http::call(
        &app,
        "POST",
        "/api/v1/player/claim",
        json!({"epoch":attached["epoch"]}),
        Some(&f.token),
    )
    .await;
    assert!(
        tokio::time::timeout(Duration::from_secs(2), state.frame())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(2), position.frame())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        observation(&app, &f.token, &attached["lease"], "state")
            .await
            .status(),
        StatusCode::CONFLICT
    );
    owner.close().await.unwrap();
}

#[tokio::test]
async fn observations_have_bounded_capacity_and_dropping_a_response_releases_its_slot() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let app = hito_server::router_with_player(f.services.clone(), Default::default(), player);
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
    let mut responses = Vec::new();
    for _ in 0..32 {
        let response = observation(&app, &f.token, &attached["lease"], "state").await;
        assert_eq!(response.status(), StatusCode::OK);
        responses.push(response);
    }
    let busy = observation(&app, &f.token, &attached["lease"], "position").await;
    assert_eq!(busy.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(busy.headers()["retry-after"], "1");
    drop(responses.pop());
    assert_eq!(
        observation(&app, &f.token, &attached["lease"], "position")
            .await
            .status(),
        StatusCode::OK
    );
    drop(responses);
    owner.close().await.unwrap();
}

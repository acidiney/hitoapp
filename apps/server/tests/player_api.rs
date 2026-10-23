use hito_core::test_support::playback as fixture;
#[path = "api/support.rs"]
#[allow(dead_code)]
mod http;
use axum::http::StatusCode;
use hito_playback::Player;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn authenticated_player_commands_replay_and_queue_pages_keep_their_lease_and_revision_fences()
{
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let app = hito_server::router_with_player(f.services.clone(), Default::default(), player);
    assert_eq!(
        http::call(&app, "GET", "/api/v1/player", Value::Null, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, inspected) =
        http::call(&app, "GET", "/api/v1/player", Value::Null, Some(&f.token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(inspected["state"].is_null());
    let (status, attachment) = http::call(
        &app,
        "POST",
        "/api/v1/player/claim",
        json!({"epoch": inspected["epoch"]}),
        Some(&f.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let command = json!({
        "id": Uuid::new_v4(), "lease": attachment["lease"], "sequence": 1, "revision": 0,
        "action": {"kind": "replace", "recordings": [f.recording, f.recording], "start": 0}
    });
    let (status, accepted) = http::call(
        &app,
        "POST",
        "/api/v1/player/commands",
        command.clone(),
        Some(&f.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accepted["revision"], 1);
    let observed = http::call(&app, "GET", "/api/v1/player", Value::Null, Some(&f.token))
        .await
        .1;
    assert_eq!(observed["state"]["current_index"], 0);
    assert_eq!(
        http::call(
            &app,
            "POST",
            "/api/v1/player/commands",
            command.clone(),
            Some(&f.token)
        )
        .await
        .1,
        accepted
    );
    let query = json!({"lease":attachment["lease"], "revision":1, "offset":0, "limit":10});
    let (status, page) = http::call(
        &app,
        "POST",
        "/api/v1/player/queue",
        query.clone(),
        Some(&f.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 2);
    assert_ne!(page["entries"][0]["id"], page["entries"][1]["id"]);
    assert_eq!(page["entries"][0]["recording"]["id"], f.recording);
    let mut stale = query;
    stale["revision"] = json!(0);
    assert_eq!(
        http::call(&app, "POST", "/api/v1/player/queue", stale, Some(&f.token))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let mut conflicting = command;
    conflicting["action"] = json!({"kind":"clear"});
    assert_eq!(
        http::call(
            &app,
            "POST",
            "/api/v1/player/commands",
            conflicting,
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    owner.close().await.unwrap();
}

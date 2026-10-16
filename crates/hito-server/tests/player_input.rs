#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "api/support.rs"]
#[allow(dead_code)]
mod http;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use axum::http::StatusCode;
use hito_playback::Player;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn unexpected_action_fields_do_not_mutate_the_queue_or_consume_command_ordering() {
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
    let mut command = json!({
        "id":Uuid::new_v4(), "lease":attached["lease"], "sequence":1, "revision":0,
        "action":{"kind":"replace", "recordings":[f.recording], "start":0}
    });
    assert_eq!(
        http::call(
            &app,
            "POST",
            "/api/v1/player/commands",
            command.clone(),
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::OK
    );
    command["id"] = json!(Uuid::new_v4());
    command["sequence"] = json!(2);
    command["revision"] = json!(1);
    for kind in [
        "clear", "resume", "pause", "next", "previous", "repeat", "shuffle",
    ] {
        command["action"] = json!({"kind":kind, "source_url":"https://private.invalid/audio"});
        assert_eq!(
            http::call(
                &app,
                "POST",
                "/api/v1/player/commands",
                command.clone(),
                Some(&f.token)
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY,
            "Unexpected fields in {kind} must be rejected"
        );
    }
    let state = http::call(&app, "GET", "/api/v1/player", Value::Null, Some(&f.token))
        .await
        .1;
    command["action"] =
        json!({"kind":"speed", "rate":1500, "source_url":"https://private.invalid/audio"});
    assert_eq!(
        http::call(
            &app,
            "POST",
            "/api/v1/player/commands",
            command.clone(),
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(state["state"]["queue_len"], 1);
    assert_eq!(state["state"]["revision"], 1);
    assert_eq!(state["state"]["desired_playing"], false);
    command["action"] = json!({"kind":"clear"});
    let (status, receipt) = http::call(
        &app,
        "POST",
        "/api/v1/player/commands",
        command,
        Some(&f.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["sequence"], 2);
    assert_eq!(receipt["revision"], 2);
    owner.close().await.unwrap();
}

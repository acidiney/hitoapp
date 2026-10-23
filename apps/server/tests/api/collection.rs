use super::support::{admin, app, call, indexed_app};
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn offline_playlist_operations_are_idempotent_and_deletion_wins() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let playlist = uuid::Uuid::new_v4().to_string();
    let create = json!({"id":uuid::Uuid::new_v4(),"action":{"kind":"playlist_create","playlist_id":playlist,"name":"Offline favorites"}});
    let batch = json!({"device_id":"desktop-a","operations":[create]});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", batch.clone(), Some(&token))
            .await
            .0,
        StatusCode::OK
    );
    let (_, replay) = call(&app, "POST", "/api/v1/sync", batch, Some(&token)).await;
    assert_eq!(replay["acknowledged"].as_array().unwrap().len(), 1);
    let (_, lists) = call(&app, "GET", "/api/v1/playlists", Value::Null, Some(&token)).await;
    assert_eq!(lists.as_array().unwrap().len(), 1);
    assert_eq!(lists[0]["name"], "Offline favorites");
    let deleted = json!({"device_id":"desktop-b","operations":[{"id":uuid::Uuid::new_v4(),"action":{"kind":"playlist_delete","playlist_id":playlist}}]});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", deleted, Some(&token))
            .await
            .0,
        StatusCode::OK
    );
    let late = json!({"device_id":"desktop-a","operations":[{"id":uuid::Uuid::new_v4(),"action":{"kind":"playlist_rename","playlist_id":playlist,"name":"Late offline edit"}}]});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", late, Some(&token))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/playlists", Value::Null, Some(&token))
            .await
            .1,
        json!([])
    );
    let (_, sync) = call(
        &app,
        "GET",
        "/api/v1/sync?cursor=0",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(sync["operations"].as_array().unwrap().len(), 3);
    let cursor = sync["cursor"].as_i64().unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/sync?cursor={cursor}"),
            Value::Null,
            Some(&token)
        )
        .await
        .1["operations"],
        json!([])
    );
}

#[tokio::test]
async fn independent_playlist_entries_merge_and_removed_entries_stay_removed() {
    let (_dir, app, token, recording) = indexed_app().await;
    let playlist = uuid::Uuid::new_v4();
    let first = uuid::Uuid::new_v4();
    let second = uuid::Uuid::new_v4();
    let ops = vec![
        json!({"id":uuid::Uuid::new_v4(),"action":{"kind":"playlist_create","playlist_id":playlist,"name":"Merged"}}),
        json!({"id":uuid::Uuid::new_v4(),"action":{"kind":"entry_insert","playlist_id":playlist,"entry_id":first,"recording_id":recording,"position":0}}),
        json!({"id":uuid::Uuid::new_v4(),"action":{"kind":"entry_insert","playlist_id":playlist,"entry_id":second,"recording_id":recording,"position":0}}),
        json!({"id":uuid::Uuid::new_v4(),"action":{"kind":"entry_remove","playlist_id":playlist,"entry_id":first}}),
        json!({"id":uuid::Uuid::new_v4(),"action":{"kind":"entry_move","playlist_id":playlist,"entry_id":first,"position":42}}),
    ];
    for operation in ops {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/sync",
                json!({"device_id":"device","operations":[operation]}),
                Some(&token)
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let (_, detail) = call(
        &app,
        "GET",
        &format!("/api/v1/playlists/{playlist}"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(detail["entries"].as_array().unwrap().len(), 1);
    assert_eq!(detail["entries"][0]["entry_id"], second.to_string());
    let reused = uuid::Uuid::new_v4();
    let batch = json!({"device_id":"device","operations":[{"id":reused,"action":{"kind":"playlist_rename","playlist_id":playlist,"name":"Accepted"}}]});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", batch.clone(), Some(&token))
            .await
            .0,
        StatusCode::OK
    );
    let mut changed = batch;
    changed["operations"][0]["action"]["name"] = json!("Different contents");
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", changed, Some(&token))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/playlists/{playlist}"),
            Value::Null,
            Some(&token)
        )
        .await
        .1["playlist"]["name"],
        "Accepted"
    );
}

#[tokio::test]
async fn likes_and_resume_positions_are_private_and_survive_new_sessions() {
    let (_dir, app, token, recording) = indexed_app().await;
    let batch = json!({"device_id":"laptop","operations":[
        {"id":uuid::Uuid::new_v4(),"action":{"kind":"like_set","recording_id":recording,"liked":true}},
        {"id":uuid::Uuid::new_v4(),"action":{"kind":"progress_set","recording_id":recording,"position_ms":700,"speed":1.25}}
    ]});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", batch, Some(&token))
            .await
            .0,
        StatusCode::OK
    );
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"admin","password":"administrator test password"}),
        None,
    )
    .await;
    let (_, activity) = call(
        &app,
        "GET",
        "/api/v1/activity",
        Value::Null,
        session["token"].as_str(),
    )
    .await;
    let (_, liked) = call(
        &app,
        "GET",
        "/api/v1/recordings?liked=true",
        Value::Null,
        session["token"].as_str(),
    )
    .await;
    assert_eq!(liked["items"][0]["id"], recording);
    assert_eq!(liked["items"][0]["liked"], true);
    assert_eq!(activity["history"][0]["position_ms"], 700);
    assert_eq!(activity["history"][0]["speed"], 1.25);
    call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"bob","password":"bob private test password"}),
        Some(&token),
    )
    .await;
    let (_, bob) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"bob","password":"bob private test password"}),
        None,
    )
    .await;
    let (_, activity) = call(
        &app,
        "GET",
        "/api/v1/activity",
        Value::Null,
        bob["token"].as_str(),
    )
    .await;
    assert_eq!(activity, json!({"history":[]}));
}

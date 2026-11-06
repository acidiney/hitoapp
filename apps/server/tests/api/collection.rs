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
async fn track_rows_carry_the_artist_page_id_and_playlist_entries_carry_album_fields() {
    let (_dir, app, token, recording) = indexed_app().await;
    let (_, recordings) = call(&app, "GET", "/api/v1/recordings", Value::Null, Some(&token)).await;
    let (_, artists) = call(&app, "GET", "/api/v1/artists", Value::Null, Some(&token)).await;
    let artist = artists["items"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(recordings["items"][0]["artist_id"], artist);
    let (status, page) = call(
        &app,
        "GET",
        &format!("/api/v1/artists/{artist}"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["recordings"]["items"][0]["artist_id"], artist);
    let playlist = uuid::Uuid::new_v4();
    let operations = vec![
        json!({"id":uuid::Uuid::new_v4(),"action":{"kind":"playlist_create","playlist_id":playlist,"name":"Linked"}}),
        json!({"id":uuid::Uuid::new_v4(),"action":{"kind":"entry_insert","playlist_id":playlist,"entry_id":uuid::Uuid::new_v4(),"recording_id":recording,"position":0}}),
    ];
    let batch = json!({"device_id":"device","operations":operations});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", batch, Some(&token))
            .await
            .0,
        StatusCode::OK
    );
    let (_, detail) = call(
        &app,
        "GET",
        &format!("/api/v1/playlists/{playlist}"),
        Value::Null,
        Some(&token),
    )
    .await;
    let entry = detail["entries"][0].as_object().unwrap();
    assert_eq!(entry["artist_id"], artist);
    assert_eq!(entry["release_id"], recordings["items"][0]["release_id"]);
    assert_eq!(entry["album"], recordings["items"][0]["album"]);
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

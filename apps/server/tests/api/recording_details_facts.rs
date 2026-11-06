use super::support::{call, indexed_app};
use axum::http::StatusCode;
use serde_json::{Value, json};

async fn sync(app: &axum::Router, token: &str, actions: Vec<Value>) {
    let operations: Vec<Value> = actions
        .into_iter()
        .map(|action| json!({"id": uuid::Uuid::new_v4(), "action": action}))
        .collect();
    let batch = json!({"device_id": "facts-test", "operations": operations});
    let (status, _) = call(app, "POST", "/api/v1/sync", batch, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
}

async fn guest(app: &axum::Router, admin: &str) -> String {
    let credentials = json!({"username": "guest", "password": "facts guest password"});
    let (_, user) = call(
        app,
        "POST",
        "/api/v1/users",
        credentials.clone(),
        Some(admin),
    )
    .await;
    let (_, libraries) = call(app, "GET", "/api/v1/libraries", Value::Null, Some(admin)).await;
    let (status, _) = call(
        app,
        "PUT",
        &format!(
            "/api/v1/libraries/{}/members/{}",
            libraries[0]["id"].as_str().unwrap(),
            user["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(admin),
    )
    .await;
    assert!(status.is_success(), "{status}");
    let (_, session) = call(app, "POST", "/api/v1/login", credentials, None).await;
    session["token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_details_wire_carries_this_accounts_last_played_and_playlists_only() {
    let (_dir, app, token, recording) = indexed_app().await;
    let other = guest(&app, &token).await;
    let playlist = uuid::Uuid::new_v4();
    sync(
        &app,
        &token,
        vec![
            json!({"kind": "progress_set", "recording_id": recording, "position_ms": 97000, "speed": 1.0}),
            json!({"kind": "playlist_create", "playlist_id": playlist, "name": "Late night"}),
            json!({"kind": "entry_insert", "playlist_id": playlist, "entry_id": uuid::Uuid::new_v4(), "recording_id": recording, "position": 0}),
        ],
    )
    .await;
    let path = format!("/api/v1/recordings/{recording}/metadata");
    let (status, own) = call(&app, "GET", &path, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(own["last_played"]["position_ms"], 97000);
    assert_eq!(own["appears_in"][0]["name"], "Late night");
    assert_eq!(own["appears_in"][0]["id"], playlist.to_string());
    assert!(own["audio_format"]["size_bytes"].as_i64().unwrap() > 0);
    assert_eq!(own["audio_format"]["format"], "WAV");
    assert!(own["release_facts"]["year"].is_null());

    let (status, foreign) = call(&app, "GET", &path, Value::Null, Some(&other)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(foreign["last_played"].is_null());
    assert_eq!(foreign["appears_in"], json!([]));
    assert!(!foreign.to_string().contains("Late night"));
}

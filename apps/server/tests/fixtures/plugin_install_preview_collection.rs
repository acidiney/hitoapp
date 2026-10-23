use super::http;
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};
use uuid::Uuid;

pub async fn seed_collection(address: SocketAddr, token: &str) {
    let libraries = http::get(address, token, "/api/v1/libraries").await;
    let library = libraries[0]["id"].as_str().unwrap();
    let response = http::call(
        address,
        "POST",
        &format!("/api/v1/libraries/{library}/scan"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let jobs = response.json();
    let job = jobs[0]["id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = http::get(address, token, &format!("/api/v1/jobs/{job}")).await;
            if status["state"] == "complete" {
                break;
            }
            assert!(status["state"] == "queued" || status["state"] == "running");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the owned audio scan must complete");
    let recordings = http::get(address, token, "/api/v1/recordings").await;
    assert_eq!(recordings["total"], 1);
    let recording = &recordings["items"][0]["id"];
    let playlist = Uuid::new_v4();
    let actions = [
        json!({"kind": "recording_title_set", "recording_id": recording, "title": "private-http-preview-title-sentinel"}),
        json!({"kind": "like_set", "recording_id": recording, "liked": true}),
        json!({"kind": "progress_set", "recording_id": recording, "position_ms": 650, "speed": 1.25}),
        json!({"kind": "playlist_create", "playlist_id": playlist, "name": "private-http-preview-playlist-sentinel"}),
        json!({"kind": "entry_insert", "playlist_id": playlist, "entry_id": Uuid::new_v4(), "recording_id": recording, "position": 0}),
    ];
    let operations: Vec<Value> = actions
        .into_iter()
        .map(|action| json!({"id": Uuid::new_v4(), "action": action}))
        .collect();
    let response = http::call(
        address,
        "POST",
        "/api/v1/sync",
        Some(json!({"device_id": "http-install-preview-client", "operations": operations})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json()["acknowledged"].as_array().unwrap().len(), 5);
    let state = account_state(address, token).await;
    assert_eq!(state["progress"][0]["position_ms"], 650);
    assert_eq!(state["progress"][0]["speed"], 1.25);
    assert_eq!(state["playlist"]["entries"].as_array().unwrap().len(), 1);
    assert!(state["recordings"]["items"][0]["liked"] == true);
}

pub async fn account_state(address: SocketAddr, token: &str) -> Value {
    let playlists = http::get(address, token, "/api/v1/playlists").await;
    let recordings = http::get(address, token, "/api/v1/recordings").await;
    let playlist = playlists[0]["id"].as_str().unwrap();
    let recording = recordings["items"][0]["id"].as_str().unwrap();
    let progress = http::call(
        address,
        "POST",
        "/api/v1/recordings/progress",
        Some(json!({"recording_ids": [recording]})),
        Some(token),
    )
    .await;
    assert_eq!(progress.status, StatusCode::OK);
    let download = http::call(
        address,
        "GET",
        &format!("/api/v1/recordings/{recording}/download"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(download.status, StatusCode::NOT_FOUND);
    json!({
        "plugins": http::get(address, token, "/api/v1/plugins").await,
        "recordings": recordings,
        "sources": http::get(address, token, &format!("/api/v1/recordings/{recording}/sources")).await,
        "progress": progress.json(),
        "playlists": playlists,
        "playlist": http::get(address, token, &format!("/api/v1/playlists/{playlist}")).await,
        "activity": http::get(address, token, "/api/v1/activity").await,
        "events": http::get(address, token, "/api/v1/sync").await
    })
}

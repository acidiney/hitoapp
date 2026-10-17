use super::{choice_http::call, http};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::net::SocketAddr;
use uuid::Uuid;

pub(super) struct CollectionSnapshot {
    playlist: String,
    detail: Value,
    metadata: Value,
    activity: Value,
}

impl CollectionSnapshot {
    pub(super) async fn prepare(address: SocketAddr, token: &str, recording: &str) -> Self {
        let playlist = Uuid::new_v4().to_string();
        let entries = [Uuid::new_v4().to_string(), Uuid::new_v4().to_string()];
        let actions = [
            json!({"kind":"playlist_create", "playlist_id":playlist, "name":"Owned duplicate playlist"}),
            json!({"kind":"entry_insert", "playlist_id":playlist, "entry_id":entries[0], "recording_id":recording, "position":0}),
            json!({"kind":"entry_insert", "playlist_id":playlist, "entry_id":entries[1], "recording_id":recording, "position":1}),
            json!({"kind":"recording_title_set", "recording_id":recording, "title":"Private corrected title"}),
            json!({"kind":"recording_artist_set", "recording_id":recording, "artist":"Private corrected performer"}),
            json!({"kind":"like_set", "recording_id":recording, "liked":true}),
            json!({"kind":"progress_set", "recording_id":recording, "position_ms":431, "speed":1.25}),
        ];
        let operations: Vec<Value> = actions
            .into_iter()
            .map(|action| json!({"id":Uuid::new_v4(), "action":action}))
            .collect();
        let receipt = call(
            address,
            "POST",
            "/api/v1/sync",
            json!({"device_id":"manual-choice-client", "operations":operations}),
            Some(token),
            StatusCode::OK,
        )
        .await;
        assert_eq!(receipt["acknowledged"].as_array().unwrap().len(), 7);
        let metadata_path = format!("/api/v1/recordings/{recording}/metadata");
        let metadata = http::get(address, token, &metadata_path).await;
        assert!(metadata["selected_observation"]["id"].is_string());
        let before_activity = http::get(address, token, "/api/v1/activity").await;
        let before_playlist =
            http::get(address, token, &format!("/api/v1/playlists/{playlist}")).await;
        Self {
            playlist,
            detail: before_playlist,
            metadata,
            activity: before_activity,
        }
    }
}

pub(super) async fn assert_collection(
    address: SocketAddr,
    token: &str,
    recording: &str,
    snapshot: &CollectionSnapshot,
    available: bool,
) {
    let playlist = &snapshot.playlist;
    let original_playlist = &snapshot.detail;
    let original_metadata = &snapshot.metadata;
    let original_activity = &snapshot.activity;
    let metadata = http::get(
        address,
        token,
        &format!("/api/v1/recordings/{recording}/metadata"),
    )
    .await;
    assert!(
        metadata["selected_observation"] == original_metadata["selected_observation"]
            && metadata["title"] == original_metadata["title"]
            && metadata["artist"] == original_metadata["artist"],
        "source choice must preserve primary and personal metadata"
    );
    let current = http::get(address, token, &format!("/api/v1/playlists/{playlist}")).await;
    assert!(
        current["playlist"] == original_playlist["playlist"]
            && current["origin"] == original_playlist["origin"]
    );
    let entries = current["entries"].as_array().unwrap();
    let before = original_playlist["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for (entry, original) in entries.iter().zip(before) {
        for field in [
            "entry_id",
            "recording_id",
            "position",
            "title",
            "artist",
            "inherited_title",
            "inherited_artist",
            "duration_ms",
            "origin",
        ] {
            assert!(
                entry[field] == original[field],
                "duplicate playlist identity and personal fields must survive source changes"
            );
        }
        assert!(entry["liked"] == true && entry["available"] == available);
    }
    let activity = http::get(address, token, "/api/v1/activity").await;
    let progress = activity["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == recording)
        .unwrap();
    let original = original_activity["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == recording)
        .unwrap();
    for field in [
        "id",
        "position_ms",
        "speed",
        "updated_at",
        "title",
        "artist",
        "liked",
    ] {
        assert!(
            progress[field] == original[field],
            "progress and private collection state must survive source changes"
        );
    }
    assert!(progress["available"] == available);
    let recordings = http::get(address, token, "/api/v1/recordings").await;
    let saved = recordings["items"].as_array().unwrap();
    assert_eq!(
        saved.len(),
        1,
        "choosing audio must not adopt a second recording"
    );
    assert!(
        saved[0]["id"] == recording
            && saved[0]["liked"] == true
            && saved[0]["available"] == available
    );
}

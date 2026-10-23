#[path = "playlist_metadata_refresh_input.rs"]
mod input;

#[path = "playlist_metadata_refresh_revision.rs"]
mod revision;

#[path = "playlist_metadata_refresh_busy.rs"]
mod busy;

use super::support::call;
use crate::playlist_metadata_fixture as fixture;
use axum::http::StatusCode;
use fixture::MetadataCopy;
use hito_core::RefreshImportedPlaylistMetadata;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn metadata_refresh_and_receipt_replay_require_the_owners_live_session() {
    let f = private_copy().await;
    let (status, owner_session) = call(
        &f.app,
        "POST",
        "/api/v1/login",
        json!({"username": "admin", "password": "administrator test password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let owner = owner_session["token"].as_str().unwrap();
    let credentials = json!({"username": "other", "password": "metadata guard account password"});
    assert_eq!(
        call(
            &f.app,
            "POST",
            "/api/v1/users",
            credentials.clone(),
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, other_session) = call(&f.app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let other = other_session["token"].as_str().unwrap();
    let other_before = account_state(&f, other).await;
    assert_eq!(other_before["catalog"]["total"], 0);
    assert_eq!(other_before["playlists"], json!([]));
    assert_eq!(other_before["activity"], json!({"history": []}));
    assert_eq!(other_before["events"]["operations"], json!([]));
    let body = request(&f);
    reject(
        &f,
        body.clone(),
        None,
        StatusCode::UNAUTHORIZED,
        "Sign in to continue",
    )
    .await;
    reject(
        &f,
        body.clone(),
        Some(other),
        StatusCode::NOT_FOUND,
        "Resource not found",
    )
    .await;
    assert_eq!(account_state(&f, other).await, other_before);

    f.publish_verified_album().await;
    let path = format!("/api/v1/playlists/{}/metadata/refresh", f.playlist);
    let (status, receipt) = call(&f.app, "POST", &path, body.clone(), Some(owner)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["request_id"], body["request_id"]);
    assert_eq!(receipt["playlist_id"], f.playlist);
    assert_eq!(receipt["updated_recordings"], 1);
    f.provider.server.offline().await;
    for attempt in [body.clone(), request(&f)] {
        reject(
            &f,
            attempt.clone(),
            None,
            StatusCode::UNAUTHORIZED,
            "Sign in to continue",
        )
        .await;
        reject(
            &f,
            attempt,
            Some(other),
            StatusCode::NOT_FOUND,
            "Resource not found",
        )
        .await;
        assert_eq!(account_state(&f, other).await, other_before);
    }

    assert_eq!(
        call(&f.app, "POST", "/api/v1/logout", Value::Null, Some(owner))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    for attempt in [body.clone(), request(&f)] {
        reject(
            &f,
            attempt,
            Some(owner),
            StatusCode::UNAUTHORIZED,
            "Sign in to continue",
        )
        .await;
    }
    let before = f.state().await;
    let observed = f.provider_requests().await;
    let (status, replay) = call(&f.app, "POST", &path, body, Some(&f.token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, receipt);
    assert_eq!(f.state().await, before);
    assert_eq!(f.provider_requests().await, observed);
    assert_eq!(account_state(&f, other).await, other_before);
}

async fn private_copy() -> MetadataCopy {
    let f = MetadataCopy::start().await;
    let operations = [
        json!({"kind": "recording_title_set", "recording_id": f.recording, "title": "Private guarded title"}),
        json!({"kind": "recording_artist_set", "recording_id": f.recording, "artist": "Private guarded performer"}),
        json!({"kind": "like_set", "recording_id": f.recording, "liked": true}),
        json!({"kind": "progress_set", "recording_id": f.recording, "position_ms": 4_200, "speed": 1.25}),
    ].map(|action| json!({"id": Uuid::new_v4(), "action": action}));
    let (status, receipt) = call(
        &f.app,
        "POST",
        "/api/v1/sync",
        json!({
            "device_id": "http-metadata-guards", "operations": operations
        }),
        Some(&f.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        receipt["acknowledged"].as_array().unwrap().len(),
        operations.len()
    );
    let state = f.state().await;
    assert_eq!(
        state["metadata"]["title"]["personal"],
        "Private guarded title"
    );
    assert_eq!(
        state["metadata"]["artist"]["personal"],
        "Private guarded performer"
    );
    assert_eq!(state["catalog"]["items"][0]["liked"], true);
    assert_eq!(state["activity"]["history"][0]["position_ms"], 4_200);
    assert_eq!(state["activity"]["history"][0]["speed"], 1.25);
    f
}

fn request(f: &MetadataCopy) -> Value {
    serde_json::to_value(RefreshImportedPlaylistMetadata {
        request_id: Uuid::new_v4(),
        installation_revision: f.provider.revision,
    })
    .unwrap()
}

async fn reject(
    f: &MetadataCopy,
    body: Value,
    token: Option<&str>,
    expected: StatusCode,
    message: &str,
) {
    let before = f.state().await;
    let observed = f.provider_requests().await;
    let path = format!("/api/v1/playlists/{}/metadata/refresh", f.playlist);
    let (status, error) = call(&f.app, "POST", &path, body, token).await;
    assert_eq!(status, expected);
    assert_eq!(error, json!({"error": message}));
    assert_eq!(f.state().await, before);
    assert_eq!(f.provider_requests().await, observed);
    assert_eq!(
        call(
            &f.app,
            "GET",
            &format!("/api/v1/recordings/{}/download", f.recording),
            Value::Null,
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

async fn account_state(f: &MetadataCopy, token: &str) -> Value {
    let mut state = serde_json::Map::new();
    for (key, path) in [
        ("catalog", "/api/v1/recordings"),
        ("playlists", "/api/v1/playlists"),
        ("activity", "/api/v1/activity"),
        ("events", "/api/v1/sync?cursor=0"),
    ] {
        let (status, body) = call(&f.app, "GET", path, Value::Null, Some(token)).await;
        assert_eq!(status, StatusCode::OK);
        state.insert(key.into(), body);
    }
    Value::Object(state)
}

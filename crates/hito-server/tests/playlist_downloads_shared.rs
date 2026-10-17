#[path = "fixtures/playlist_download_fixture.rs"]
#[allow(dead_code)]
mod fixture;
#[path = "fixtures/download_host.rs"]
mod host;
#[path = "fixtures/artwork_http.rs"]
mod http;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use axum::http::{HeaderMap, StatusCode};
use fixture::{Fixture, Selection, wait_for_json};
use futures_util::FutureExt;
use http::{call, get};
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe};
use uuid::Uuid;

#[tokio::test]
async fn playlist_and_direct_retentions_share_one_copy_and_old_deletes_cannot_remove_a_new_root() {
    let fixture = Fixture::start().await;
    let mut host = host::Host::start(fixture.services.clone()).await;
    let outcome = AssertUnwindSafe(exercise(&fixture, host.address))
        .catch_unwind()
        .await;
    host.request_shutdown();
    assert!(
        host.join().await,
        "the owned host must join HTTP and all background work"
    );
    assert!(
        std::fs::read(fixture.directory.path().join("music/original.wav")).unwrap()
            == fixture.original,
        "removing shared download roots must preserve user-owned original bytes"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(fixture: &Fixture, address: SocketAddr) {
    let selection = fixture.prepare(address).await;
    let token = &selection.token;
    let follow_path = format!("/api/v1/playlists/{}", selection.playlist);
    let personal = selection.personal_playlist.to_string();
    let before = fixture.saved_state(address, &selection).await;
    assert_eq!(before["catalog"]["total"], 1);
    assert!(before["metadata"]["title"]["personal"] == "My retained title");
    assert!(before["metadata"]["artist"]["personal"] == "My retained artist");
    assert!(before["activity"]["history"][0]["position_ms"] == 700);
    assert!(before["activity"]["history"][0]["speed"] == 1.25);
    let followed = get(address, token, &follow_path).await;
    assert!(followed["playlist"]["kind"] == "followed");
    let follow_entries = entry_ids(&followed, &selection.recording);
    let personal_entries = entry_ids(&before["playlist"], &selection.recording);
    assert_eq!(follow_entries.len(), 2);
    assert_ne!(follow_entries[0], follow_entries[1]);
    assert_eq!(personal_entries.len(), 1);
    assert!(followed["entries"][0]["origin"]["occurrence_id"] == "first");
    assert!(followed["entries"][1]["origin"]["occurrence_id"] == "duplicate");
    assert!(fixture.provider.requests("v1/audio").await.is_empty());

    let track_path = format!("/api/v1/recordings/{}/download", selection.recording);
    let follow_download = format!("{follow_path}/download");
    let personal_download = format!("/api/v1/playlists/{personal}/download");
    for path in [&track_path, &follow_download, &personal_download] {
        assert_absent(address, token, path).await;
    }
    let old_request = json!({"request_id": Uuid::new_v4()});
    let old_receipt = download(address, token, &track_path, &old_request).await;
    assert!(old_receipt["recording_id"] == selection.recording);
    let old_root = old_receipt["intent_id"].as_str().unwrap();
    Uuid::parse_str(old_root).unwrap();
    let ready = wait_for_json(address, token, &track_path, |state| {
        assert!(matches!(
            state["status"].as_str(),
            Some("queued" | "running" | "verifying" | "ready")
        ));
        state["status"] == "ready"
    })
    .await;
    assert!(ready["intent_id"] == old_root);
    assert!(ready["recording_id"] == selection.recording);
    assert!(ready["host_device_id"] == fixture::HOST);
    let source = ready["source_id"].as_str().unwrap();
    Uuid::parse_str(source).unwrap();
    assert_ne!(source, selection.provider_source);
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
    fixture
        .provider
        .audio(
            StatusCode::SERVICE_UNAVAILABLE,
            HeaderMap::new(),
            Vec::new(),
        )
        .await;
    assert_audio(address, token, source, &fixture.audio).await;

    let follow_request = json!({"request_id": Uuid::new_v4()});
    let follow_receipt = download(address, token, &follow_download, &follow_request).await;
    let personal_request = json!({"request_id": Uuid::new_v4()});
    let personal_receipt = download(address, token, &personal_download, &personal_request).await;
    assert!(follow_receipt["playlist_id"] == selection.playlist);
    assert!(personal_receipt["playlist_id"] == personal);
    let follow_root = follow_receipt["retention_id"].as_str().unwrap();
    let personal_root = personal_receipt["retention_id"].as_str().unwrap();
    for root in [follow_root, personal_root] {
        Uuid::parse_str(root).unwrap();
        assert_ne!(root, old_root);
    }
    assert_ne!(follow_root, personal_root);
    assert_playlist_saved(
        address,
        &selection,
        &selection.playlist,
        &follow_entries,
        source,
        follow_root,
    )
    .await;
    assert_playlist_saved(
        address,
        &selection,
        &personal,
        &personal_entries,
        source,
        personal_root,
    )
    .await;
    assert!(download(address, token, &follow_download, &follow_request).await == follow_receipt);
    assert!(
        download(address, token, &personal_download, &personal_request).await == personal_receipt
    );
    assert_preserved(fixture, address, &selection, &before, &follow_entries).await;

    remove(address, token, old_root).await;
    assert_absent(address, token, &track_path).await;
    assert_audio(address, token, source, &fixture.audio).await;
    assert!(download(address, token, &track_path, &old_request).await == old_receipt);
    assert_absent(address, token, &track_path).await;

    let fresh_request = json!({"request_id": Uuid::new_v4()});
    let fresh_receipt = download(address, token, &track_path, &fresh_request).await;
    assert!(fresh_receipt["recording_id"] == selection.recording);
    let fresh_root = fresh_receipt["intent_id"].as_str().unwrap();
    Uuid::parse_str(fresh_root).unwrap();
    assert_ne!(fresh_root, old_root);
    assert_ne!(fresh_root, follow_root);
    assert_ne!(fresh_root, personal_root);
    assert_direct_saved(address, &selection, fresh_root, source).await;
    remove(address, token, old_root).await;
    remove(address, token, old_root).await;
    assert!(download(address, token, &track_path, &old_request).await == old_receipt);
    assert!(download(address, token, &track_path, &fresh_request).await == fresh_receipt);
    assert_direct_saved(address, &selection, fresh_root, source).await;
    assert_playlist_saved(
        address,
        &selection,
        &selection.playlist,
        &follow_entries,
        source,
        follow_root,
    )
    .await;
    assert_playlist_saved(
        address,
        &selection,
        &personal,
        &personal_entries,
        source,
        personal_root,
    )
    .await;
    assert_audio(address, token, source, &fixture.audio).await;
    assert_preserved(fixture, address, &selection, &before, &follow_entries).await;

    remove(address, token, follow_root).await;
    assert_absent(address, token, &follow_download).await;
    assert!(download(address, token, &follow_download, &follow_request).await == follow_receipt);
    assert_absent(address, token, &follow_download).await;
    assert_playlist_saved(
        address,
        &selection,
        &personal,
        &personal_entries,
        source,
        personal_root,
    )
    .await;
    assert_direct_saved(address, &selection, fresh_root, source).await;
    assert_audio(address, token, source, &fixture.audio).await;
    assert_preserved(fixture, address, &selection, &before, &follow_entries).await;

    remove(address, token, personal_root).await;
    assert_absent(address, token, &personal_download).await;
    assert!(
        download(address, token, &personal_download, &personal_request).await == personal_receipt
    );
    assert_absent(address, token, &personal_download).await;
    assert_direct_saved(address, &selection, fresh_root, source).await;
    assert_audio(address, token, source, &fixture.audio).await;
    assert_preserved(fixture, address, &selection, &before, &follow_entries).await;

    remove(address, token, fresh_root).await;
    assert_absent(address, token, &track_path).await;
    let audio_path = format!("/api/v1/sources/{source}/audio");
    assert_absent(address, token, &audio_path).await;
    let sources = get(
        address,
        token,
        &format!("/api/v1/recordings/{}/sources", selection.recording),
    )
    .await;
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert!(sources[0]["id"] == selection.provider_source);
    assert!(download(address, token, &track_path, &old_request).await == old_receipt);
    assert!(download(address, token, &track_path, &fresh_request).await == fresh_receipt);
    for path in [
        &track_path,
        &follow_download,
        &personal_download,
        &audio_path,
    ] {
        assert_absent(address, token, path).await;
    }
    assert_preserved(fixture, address, &selection, &before, &follow_entries).await;
}

async fn download(address: SocketAddr, token: &str, path: &str, request: &Value) -> Value {
    let response = call(address, "POST", path, Some(request.clone()), Some(token)).await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let receipt = response.json();
    assert!(receipt["request_id"] == request["request_id"]);
    receipt
}

async fn remove(address: SocketAddr, token: &str, root: &str) {
    let response = call(
        address,
        "DELETE",
        &format!("/api/v1/downloads/{root}"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
}

async fn assert_absent(address: SocketAddr, token: &str, path: &str) {
    assert_eq!(
        call(address, "GET", path, None, Some(token)).await.status,
        StatusCode::NOT_FOUND
    );
}

async fn assert_audio(address: SocketAddr, token: &str, source: &str, expected: &[u8]) {
    let response = call(
        address,
        "GET",
        &format!("/api/v1/sources/{source}/audio"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["content-type"], "audio/wav");
    assert!(
        response.body == expected,
        "all live roots must retain the same complete owned WAV"
    );
}

async fn assert_direct_saved(address: SocketAddr, selection: &Selection, root: &str, source: &str) {
    let state = get(
        address,
        &selection.token,
        &format!("/api/v1/recordings/{}/download", selection.recording),
    )
    .await;
    assert!(state["intent_id"] == root);
    assert!(state["recording_id"] == selection.recording);
    assert!(state["host_device_id"] == fixture::HOST);
    assert!(state["status"] == "ready");
    assert!(state["source_id"] == source);
    assert!(state["error"].is_null());
}

async fn assert_playlist_saved(
    address: SocketAddr,
    selection: &Selection,
    playlist: &str,
    entries: &[String],
    source: &str,
    root: &str,
) {
    let path = format!("/api/v1/playlists/{playlist}/download");
    let state = wait_for_json(address, &selection.token, &path, |state| {
        state["desired_revision"]
            .as_u64()
            .is_some_and(|revision| revision > 0)
            && state["desired_revision"] == state["reconciled_revision"]
            && state["entries"]["items"].as_array().is_some_and(|items| {
                items.len() == entries.len() && items.iter().all(|item| item["status"] == "ready")
            })
    })
    .await;
    assert!(state["retention_id"] == root);
    assert!(state["playlist_id"] == playlist);
    assert!(state["host_device_id"] == fixture::HOST);
    assert!(state["entries"]["next_cursor"].is_null());
    let mut actual: Vec<&str> = state["entries"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            assert!(entry["recording_id"] == selection.recording);
            assert!(entry["source_id"] == source);
            assert!(entry["error"].is_null());
            entry["entry_id"].as_str().unwrap()
        })
        .collect();
    actual.sort_unstable();
    let mut expected: Vec<&str> = entries.iter().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

fn entry_ids(detail: &Value, recording: &str) -> Vec<String> {
    detail["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            assert!(entry["recording_id"] == recording);
            let id = entry["entry_id"].as_str().unwrap();
            Uuid::parse_str(id).unwrap();
            id.to_owned()
        })
        .collect()
}

async fn assert_preserved(
    fixture: &Fixture,
    address: SocketAddr,
    selection: &Selection,
    before: &Value,
    follow_entries: &[String],
) {
    assert!(fixture.saved_state(address, selection).await == *before);
    let followed = get(
        address,
        &selection.token,
        &format!("/api/v1/playlists/{}", selection.playlist),
    )
    .await;
    assert_eq!(
        entry_ids(&followed, &selection.recording).as_slice(),
        follow_entries
    );
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
}

#[path = "../fixtures/playlist_download_fixture.rs"]
mod fixture;
#[path = "../fixtures/download_host.rs"]
mod host;
#[path = "../fixtures/artwork_http.rs"]
mod http;
use hito_core::test_support::content_service as provider;
use hito_core::test_support::support;

use axum::http::{HeaderMap, StatusCode};
use fixture::{Fixture, Selection, wait_for_json};
use futures_util::FutureExt;
use http::{call, get};
use serde_json::json;
use std::{net::SocketAddr, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn a_downloaded_follow_retains_one_copy_until_its_last_authoritative_occurrence_is_removed() {
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
        "retention changes must preserve the user-owned original bytes"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(fixture: &Fixture, address: SocketAddr) {
    let selection = fixture.prepare(address).await;
    let token = selection.token.as_str();
    let detail_path = format!("/api/v1/playlists/{}", selection.playlist);
    let detail = get(address, token, &detail_path).await;
    assert!(detail["playlist"]["kind"] == "followed");
    let entries = detail["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let entry_ids: Vec<String> = entries
        .iter()
        .map(|entry| {
            assert!(entry["recording_id"] == selection.recording);
            let id = entry["entry_id"].as_str().unwrap();
            Uuid::parse_str(id).unwrap();
            id.to_owned()
        })
        .collect();
    assert_ne!(entry_ids[0], entry_ids[1]);
    assert!(entries[0]["origin"]["occurrence_id"] == "first");
    assert!(entries[1]["origin"]["occurrence_id"] == "duplicate");
    let before = fixture.saved_state(address, &selection).await;
    assert_eq!(before["catalog"]["total"], 1);
    assert_eq!(before["playlist"]["entries"].as_array().unwrap().len(), 1);
    assert!(before["metadata"]["title"]["personal"] == "My retained title");
    assert!(before["metadata"]["artist"]["personal"] == "My retained artist");
    assert!(before["activity"]["history"][0]["position_ms"] == 700);
    assert!(before["activity"]["history"][0]["speed"] == 1.25);
    assert!(fixture.provider.requests("v1/audio").await.is_empty());

    let playlist_path = format!("{detail_path}/download");
    assert_eq!(
        call(address, "GET", &playlist_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let track_path = format!("/api/v1/recordings/{}/download", selection.recording);
    assert_eq!(
        call(address, "GET", &track_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let track_request = json!({"request_id": Uuid::new_v4()});
    let response = call(
        address,
        "POST",
        &track_path,
        Some(track_request.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let track_receipt = response.json();
    assert!(track_receipt["request_id"] == track_request["request_id"]);
    assert!(track_receipt["recording_id"] == selection.recording);
    let track_intent = track_receipt["intent_id"].as_str().unwrap();
    Uuid::parse_str(track_intent).unwrap();
    let ready = wait_for_json(address, token, &track_path, |state| {
        assert!(matches!(
            state["status"].as_str(),
            Some("queued" | "running" | "verifying" | "ready")
        ));
        state["status"] == "ready"
    })
    .await;
    assert!(ready["intent_id"] == track_intent);
    assert!(ready["host_device_id"] == fixture::HOST);
    let source = ready["source_id"].as_str().unwrap();
    Uuid::parse_str(source).unwrap();
    assert_ne!(source, selection.provider_source);
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
    let audio_path = format!("/api/v1/sources/{source}/audio");
    fixture
        .provider
        .audio(
            StatusCode::SERVICE_UNAVAILABLE,
            HeaderMap::new(),
            Vec::new(),
        )
        .await;

    let playlist_request = json!({"request_id": Uuid::new_v4()});
    let response = call(
        address,
        "POST",
        &playlist_path,
        Some(playlist_request.clone()),
        Some(token),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::ACCEPTED,
        "an explicit playlist Download must retain the existing owned copy"
    );
    let playlist_receipt = response.json();
    assert!(playlist_receipt["request_id"] == playlist_request["request_id"]);
    assert!(playlist_receipt["playlist_id"] == selection.playlist);
    let retention = playlist_receipt["retention_id"].as_str().unwrap();
    Uuid::parse_str(retention).unwrap();
    assert_ne!(retention, track_intent);
    assert_playlist_saved(address, &selection, &entry_ids, source, retention).await;
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
    assert!(fixture.saved_state(address, &selection).await == before);

    let response = call(
        address,
        "DELETE",
        &format!("/api/v1/downloads/{track_intent}"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(address, "GET", &track_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let response = call(address, "GET", &audio_path, None, Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["content-type"], "audio/wav");
    assert!(
        response.body == fixture.audio,
        "the playlist must retain the same complete owned audio"
    );
    assert_playlist_saved(address, &selection, &entry_ids, source, retention).await;
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);

    fixture
        .refresh(address, &selection, "one-occurrence", &["duplicate"])
        .await;
    let detail = get(address, token, &detail_path).await;
    let entries = detail["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0]["entry_id"] == entry_ids[1]);
    assert!(entries[0]["recording_id"] == selection.recording);
    assert_playlist_saved(address, &selection, &entry_ids[1..], source, retention).await;
    let response = call(address, "GET", &audio_path, None, Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(response.body == fixture.audio);
    assert!(fixture.saved_state(address, &selection).await == before);
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);

    fixture
        .refresh(address, &selection, "empty-selection", &[])
        .await;
    let detail = get(address, token, &detail_path).await;
    assert!(detail["entries"].as_array().unwrap().is_empty());
    assert_playlist_saved(address, &selection, &[], source, retention).await;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let response = call(address, "GET", &audio_path, None, Some(token)).await;
            if response.status == StatusCode::NOT_FOUND {
                break;
            }
            assert_eq!(response.status, StatusCode::OK);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("removing the last authoritative occurrence must release managed playback");
    let sources = get(
        address,
        token,
        &format!("/api/v1/recordings/{}/sources", selection.recording),
    )
    .await;
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert!(sources[0]["id"] == selection.provider_source);
    let response = call(
        address,
        "POST",
        &track_path,
        Some(track_request),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert!(
        response.json() == track_receipt,
        "the removed track receipt must remain immutable"
    );
    assert_eq!(
        call(address, "GET", &track_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(address, "GET", &audio_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert!(fixture.saved_state(address, &selection).await == before);
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
}

async fn assert_playlist_saved(
    address: SocketAddr,
    selection: &Selection,
    entry_ids: &[String],
    source: &str,
    retention: &str,
) {
    let path = format!("/api/v1/playlists/{}/download", selection.playlist);
    let state = wait_for_json(address, &selection.token, &path, |state| {
        state["desired_revision"]
            .as_u64()
            .is_some_and(|revision| revision > 0)
            && state["desired_revision"] == state["reconciled_revision"]
            && state["entries"]["items"].as_array().is_some_and(|items| {
                items.len() == entry_ids.len() && items.iter().all(|item| item["status"] == "ready")
            })
    })
    .await;
    assert!(state["retention_id"] == retention);
    assert!(state["playlist_id"] == selection.playlist);
    assert!(state["host_device_id"] == fixture::HOST);
    assert!(state["entries"]["next_cursor"].is_null());
    let mut actual_ids: Vec<&str> = state["entries"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            assert!(item["recording_id"] == selection.recording);
            assert!(item["source_id"] == source);
            assert!(item["error"].is_null());
            item["entry_id"].as_str().unwrap()
        })
        .collect();
    actual_ids.sort_unstable();
    let mut expected_ids: Vec<&str> = entry_ids.iter().map(String::as_str).collect();
    expected_ids.sort_unstable();
    assert_eq!(actual_ids, expected_ids);
}

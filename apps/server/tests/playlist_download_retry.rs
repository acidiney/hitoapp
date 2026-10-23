#[path = "fixtures/playlist_download_fixture.rs"]
#[allow(dead_code)]
mod fixture;
#[path = "fixtures/download_host.rs"]
mod host;
#[path = "fixtures/artwork_http.rs"]
mod http;
use hito_core::test_support::content_service as provider;
use hito_core::test_support::support;

use axum::http::{HeaderMap, StatusCode};
use fixture::{Fixture, Selection};
use futures_util::FutureExt;
use http::{call, get};
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn a_fresh_playlist_download_retries_failed_duplicates_without_an_individual_retention() {
    let fixture = Fixture::start().await;
    fixture
        .provider
        .audio(
            StatusCode::SERVICE_UNAVAILABLE,
            HeaderMap::new(),
            Vec::new(),
        )
        .await;
    let mut host = host::Host::start(fixture.services.clone()).await;
    let outcome = AssertUnwindSafe(exercise(&fixture, host.address))
        .catch_unwind()
        .await;
    host.request_shutdown();
    assert!(
        host.join().await,
        "the owned host must join HTTP and background work"
    );
    assert!(
        std::fs::read(fixture.directory.path().join("music/original.wav")).unwrap()
            == fixture.original,
        "playlist retry must preserve the user-owned original bytes"
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
    assert_no_individual_download(address, &selection).await;
    assert!(fixture.provider.requests("v1/audio").await.is_empty());

    let download_path = format!("{detail_path}/download");
    let request_a = json!({"request_id": Uuid::new_v4()});
    let receipt_a = download(address, token, &download_path, request_a.clone()).await;
    assert!(receipt_a["request_id"] == request_a["request_id"]);
    assert!(receipt_a["playlist_id"] == selection.playlist);
    let retention = receipt_a["retention_id"].as_str().unwrap();
    Uuid::parse_str(retention).unwrap();
    let failed = wait_for_entries(address, &selection, &entry_ids, "failed").await;
    assert!(failed["retention_id"] == retention);
    for item in failed["entries"]["items"].as_array().unwrap() {
        assert!(item["error"] == "transfer_failed");
        assert!(item["source_id"].is_null());
    }
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 3);
    assert_no_individual_download(address, &selection).await;

    let replay = download(address, token, &download_path, request_a).await;
    assert!(
        replay == receipt_a,
        "the original playlist receipt must stay immutable"
    );
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(get(address, token, &download_path).await == failed);
    assert_eq!(
        fixture.provider.requests("v1/audio").await.len(),
        3,
        "replaying the old UUID must not rearm a failed playlist transfer"
    );
    assert!(fixture.saved_state(address, &selection).await == before);

    fixture
        .provider
        .audio(
            StatusCode::OK,
            HeaderMap::from_iter([
                (
                    "content-type".parse().unwrap(),
                    "audio/wav".parse().unwrap(),
                ),
                (
                    "content-length".parse().unwrap(),
                    fixture.audio.len().to_string().parse().unwrap(),
                ),
            ]),
            fixture.audio.clone(),
        )
        .await;
    let request_b = json!({"request_id": Uuid::new_v4()});
    let receipt_b = download(address, token, &download_path, request_b.clone()).await;
    assert!(receipt_b["request_id"] == request_b["request_id"]);
    assert!(receipt_b["playlist_id"] == selection.playlist);
    assert!(
        receipt_b["retention_id"] == retention,
        "a fresh retry must retain the existing playlist root"
    );
    let ready = wait_for_entries(address, &selection, &entry_ids, "ready").await;
    assert!(ready["retention_id"] == retention);
    assert!(ready["desired_revision"] == failed["desired_revision"]);
    let items = ready["entries"]["items"].as_array().unwrap();
    let source = items[0]["source_id"].as_str().unwrap();
    Uuid::parse_str(source).unwrap();
    assert_ne!(source, selection.provider_source);
    for item in items {
        assert!(item["source_id"] == source);
        assert!(item["error"].is_null());
    }
    assert_eq!(
        fixture.provider.requests("v1/audio").await.len(),
        4,
        "both stable duplicates must share one successful replacement transfer"
    );
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
        response.body == fixture.audio,
        "managed playback must serve the complete owned WAV"
    );
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 4);
    assert_no_individual_download(address, &selection).await;
    assert!(get(address, token, &detail_path).await == detail);
    assert!(fixture.saved_state(address, &selection).await == before);
}

#[tokio::test]
async fn a_failed_playlist_retry_needs_another_fresh_command_before_it_can_transfer_again() {
    let fixture = Fixture::start().await;
    fixture
        .provider
        .audio(
            StatusCode::SERVICE_UNAVAILABLE,
            HeaderMap::new(),
            Vec::new(),
        )
        .await;
    let mut host = host::Host::start(fixture.services.clone()).await;
    let outcome = AssertUnwindSafe(exercise_failed_retry(&fixture, host.address))
        .catch_unwind()
        .await;
    host.request_shutdown();
    assert!(
        host.join().await,
        "the owned host must join HTTP and background work"
    );
    assert!(
        std::fs::read(fixture.directory.path().join("music/original.wav")).unwrap()
            == fixture.original,
        "a failed replacement and later recovery must preserve original bytes"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise_failed_retry(fixture: &Fixture, address: SocketAddr) {
    let selection = fixture.prepare(address).await;
    let token = selection.token.as_str();
    let detail_path = format!("/api/v1/playlists/{}", selection.playlist);
    let detail = get(address, token, &detail_path).await;
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
    let before = fixture.saved_state(address, &selection).await;
    assert_no_individual_download(address, &selection).await;
    assert!(fixture.provider.requests("v1/audio").await.is_empty());

    let download_path = format!("{detail_path}/download");
    let receipt_a = download(
        address,
        token,
        &download_path,
        json!({"request_id": Uuid::new_v4()}),
    )
    .await;
    let retention = receipt_a["retention_id"].as_str().unwrap();
    Uuid::parse_str(retention).unwrap();
    let original_failure = wait_for_entries(address, &selection, &entry_ids, "failed").await;
    assert!(original_failure["retention_id"] == retention);
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 3);

    let request_b = json!({"request_id": Uuid::new_v4()});
    let receipt_b = download(address, token, &download_path, request_b.clone()).await;
    assert!(receipt_b["request_id"] == request_b["request_id"]);
    assert!(receipt_b["playlist_id"] == selection.playlist);
    assert!(receipt_b["retention_id"] == retention);
    wait_for_audio_attempts(fixture, 6).await;
    let replacement_failure = wait_for_entries(address, &selection, &entry_ids, "failed").await;
    assert!(replacement_failure == original_failure);
    for item in replacement_failure["entries"]["items"].as_array().unwrap() {
        assert!(item["error"] == "transfer_failed");
        assert!(item["source_id"].is_null());
    }
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 6);
    assert_no_individual_download(address, &selection).await;

    let replay_b = download(address, token, &download_path, request_b).await;
    assert!(
        replay_b == receipt_b,
        "the accepted replacement receipt must remain immutable after its transfer fails"
    );
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(get(address, token, &download_path).await == replacement_failure);
    assert_eq!(
        fixture.provider.requests("v1/audio").await.len(),
        6,
        "duplicates and replay of the accepted retry UUID must not resubmit its newer failure"
    );
    assert!(get(address, token, &detail_path).await == detail);
    assert!(fixture.saved_state(address, &selection).await == before);

    fixture
        .provider
        .audio(
            StatusCode::OK,
            HeaderMap::from_iter([
                (
                    "content-type".parse().unwrap(),
                    "audio/wav".parse().unwrap(),
                ),
                (
                    "content-length".parse().unwrap(),
                    fixture.audio.len().to_string().parse().unwrap(),
                ),
            ]),
            fixture.audio.clone(),
        )
        .await;
    let request_c = json!({"request_id": Uuid::new_v4()});
    let receipt_c = download(address, token, &download_path, request_c.clone()).await;
    assert!(receipt_c["request_id"] == request_c["request_id"]);
    assert!(receipt_c["playlist_id"] == selection.playlist);
    assert!(receipt_c["retention_id"] == retention);
    let ready = wait_for_entries(address, &selection, &entry_ids, "ready").await;
    assert!(ready["retention_id"] == retention);
    assert!(ready["desired_revision"] == original_failure["desired_revision"]);
    let items = ready["entries"]["items"].as_array().unwrap();
    let source = items[0]["source_id"].as_str().unwrap();
    Uuid::parse_str(source).unwrap();
    assert_ne!(source, selection.provider_source);
    for item in items {
        assert!(item["source_id"] == source);
        assert!(item["error"].is_null());
    }
    assert_eq!(
        fixture.provider.requests("v1/audio").await.len(),
        7,
        "a fresh command must recover both duplicates with one successful transfer"
    );
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
    assert!(response.body == fixture.audio);
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 7);
    assert_no_individual_download(address, &selection).await;
    assert!(get(address, token, &detail_path).await == detail);
    assert!(fixture.saved_state(address, &selection).await == before);
}

async fn wait_for_audio_attempts(fixture: &Fixture, expected: usize) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let attempts = fixture.provider.requests("v1/audio").await.len();
            assert!(
                attempts <= expected,
                "playlist retry exceeded its provider attempt budget"
            );
            if attempts == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the replacement must make its bounded provider attempts");
}

async fn download(address: SocketAddr, token: &str, path: &str, request: Value) -> Value {
    let response = call(address, "POST", path, Some(request), Some(token)).await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    response.json()
}

async fn wait_for_entries(
    address: SocketAddr,
    selection: &Selection,
    entry_ids: &[String],
    expected_status: &str,
) -> Value {
    let path = format!("/api/v1/playlists/{}/download", selection.playlist);
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let state = get(address, &selection.token, &path).await;
            assert!(state["playlist_id"] == selection.playlist);
            assert!(state["host_device_id"] == fixture::HOST);
            assert!(state["entries"]["next_cursor"].is_null());
            let items = state["entries"]["items"].as_array().unwrap();
            if state["desired_revision"]
                .as_u64()
                .is_some_and(|revision| revision > 0)
                && state["desired_revision"] == state["reconciled_revision"]
                && items.len() == entry_ids.len()
                && entry_ids.iter().all(|entry_id| {
                    items.iter().any(|item| {
                        item["entry_id"] == *entry_id
                            && item["recording_id"] == selection.recording
                            && item["status"] == expected_status
                    })
                })
            {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    result.unwrap_or_else(|_| {
        panic!("both stable playlist occurrences must become {expected_status}")
    })
}

async fn assert_no_individual_download(address: SocketAddr, selection: &Selection) {
    let response = call(
        address,
        "GET",
        &format!("/api/v1/recordings/{}/download", selection.recording),
        None,
        Some(&selection.token),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "playlist retry must not create an individual retention"
    );
}

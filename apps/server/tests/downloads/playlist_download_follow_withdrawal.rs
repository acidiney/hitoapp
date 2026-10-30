#[path = "../fixtures/playlist_download_availability_fixture.rs"]
mod fixture;
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod http;
use hito_core::test_support::playlist_service as provider;
use hito_core::test_support::support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use fixture::Fixture;
use hito_core::{
    DownloadRequest, Error, PlaylistDownloadEntryState, PlaylistDownloadQuery,
    PlaylistDownloadStatus, RefreshFollowedPlaylist,
};
use http_body_util::BodyExt;
use serde_json::json;
use std::{future::pending, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn a_follow_entry_becoming_unavailable_reclaims_its_previous_managed_recording() {
    let fixture = Fixture::start().await;
    fixture::snapshot(&fixture.provider, "initial-ready-entry", true).await;
    refresh(&fixture).await;
    let initial = fixture
        .services
        .collection
        .playlist(&fixture.actor, &fixture.playlist)
        .await
        .unwrap();
    assert_eq!(initial.entries.len(), 1);
    let stable_entry = initial.entries[0].entry_id.clone();
    assert_eq!(
        initial.entries[0].recording_id.as_deref(),
        Some(fixture.recording.as_str())
    );
    let preserved = fixture.saved_state().await;
    assert_eq!(preserved["catalog"]["total"], 2);
    assert_eq!(preserved["copy"]["entries"].as_array().unwrap().len(), 1);
    assert!(preserved["metadata"]["title"]["personal"] == "My private title");
    assert!(preserved["metadata"]["artist"]["personal"] == "My private artist");
    assert!(preserved["activity"]["history"][0]["position_ms"] == 700);
    assert!(preserved["activity"]["history"][0]["speed"] == 1.25);
    let receipt = fixture
        .services
        .downloads
        .create_playlist(
            &fixture.actor,
            &fixture.playlist,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    let ready = advance_until(&fixture, 1, PlaylistDownloadEntryState::Ready).await;
    assert_eq!(ready.retention_id, receipt.retention_id);
    assert_eq!(ready.entries.items[0].entry_id, stable_entry);
    assert!(matches!(
        fixture
            .services
            .downloads
            .status(&fixture.actor, &fixture.recording)
            .await,
        Err(Error::NotFound)
    ));
    let managed_source = ready.entries.items[0].source_id.as_deref().unwrap();
    Uuid::parse_str(managed_source).unwrap();
    let files = owned_audio(&fixture);
    assert_eq!(files.len(), 1, "the playlist must own one managed copy");
    assert!(
        files[0] == fixture.audio,
        "the owned file must contain the verified fixture audio"
    );
    let response = audio(&fixture, managed_source).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.as_ref() == fixture.audio.as_slice());
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
    assert_eq!(fixture.saved_state().await, preserved);

    provider::playlist(
        &fixture.provider,
        fixture::PLAYLIST,
        "stable-entry-withdrawn-with-new-unknown",
        vec![
            json!({"occurrence_id": fixture::OCCURRENCE, "item": {
                "kind": "unavailable", "provider_recording_id": fixture::RECORDING,
                "title": "Temporarily unavailable", "artist": "Owned artist"
            }}),
            json!({"occurrence_id": "new-unknown-entry", "item": {"kind": "unknown"}}),
        ],
    )
    .await;
    refresh(&fixture).await;
    let withdrawn = fixture
        .services
        .collection
        .playlist(&fixture.actor, &fixture.playlist)
        .await
        .unwrap();
    assert_eq!(withdrawn.entries.len(), 2);
    let stable = withdrawn
        .entries
        .iter()
        .find(|entry| entry.entry_id == stable_entry)
        .unwrap();
    assert!(stable.recording_id.is_none());
    let origin = stable.origin.as_ref().unwrap();
    assert_eq!(origin.occurrence_id.as_deref(), Some(fixture::OCCURRENCE));
    assert_eq!(
        origin.provider_recording_id.as_deref(),
        Some(fixture::RECORDING)
    );
    assert!(
        withdrawn
            .entries
            .iter()
            .any(|entry| entry.entry_id != stable_entry)
    );
    let reconciled = advance_until(&fixture, 2, PlaylistDownloadEntryState::Unavailable).await;
    assert!(reconciled.desired_revision > ready.desired_revision);
    assert_eq!(reconciled.retention_id, receipt.retention_id);
    assert!(
        reconciled
            .entries
            .items
            .iter()
            .all(|entry| entry.recording_id.is_none())
    );
    assert_eq!(
        audio(&fixture, managed_source).await.status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        owned_audio(&fixture).is_empty(),
        "replacing the projected recording must reclaim its owned bytes after reconciliation and cleanup"
    );
    let sources = fixture
        .get(&format!("/api/v1/recordings/{}/sources", fixture.recording))
        .await;
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert!(sources[0]["provider"] == "plugin");
    assert_eq!(fixture.saved_state().await, preserved);

    fixture
        .services
        .downloads
        .remove(&fixture.actor, &receipt.retention_id)
        .await
        .unwrap();
    for _ in 0..4 {
        fixture
            .services
            .downloads
            .run_next_playlist_reconciliation_until(pending())
            .await
            .unwrap();
        fixture
            .services
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap();
    }
    assert!(matches!(
        fixture
            .services
            .downloads
            .playlist_status(
                &fixture.actor,
                &fixture.playlist,
                PlaylistDownloadQuery::default()
            )
            .await,
        Err(Error::NotFound)
    ));
    fixture.services.downloads.finish_downloads().await.unwrap();
    assert!(
        owned_audio(&fixture).is_empty(),
        "removing the retention must leave no orphaned managed bytes"
    );
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
    assert_eq!(fixture.saved_state().await, preserved);
    assert!(
        std::fs::read(fixture.directory.path().join("music/original.wav")).unwrap()
            == fixture.original
    );
}

async fn refresh(fixture: &Fixture) {
    fixture
        .services
        .collection
        .enqueue_follow_refresh(
            &fixture.actor,
            &fixture.playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert!(
        fixture
            .services
            .collection
            .run_next_follow_refresh_until(pending())
            .await
            .unwrap()
    );
}

async fn advance_until(
    fixture: &Fixture,
    count: usize,
    expected: PlaylistDownloadEntryState,
) -> PlaylistDownloadStatus {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            fixture
                .services
                .downloads
                .run_next_playlist_reconciliation_until(pending())
                .await
                .unwrap();
            fixture
                .services
                .downloads
                .run_next_job_until(pending())
                .await
                .unwrap();
            let state = fixture
                .services
                .downloads
                .playlist_status(
                    &fixture.actor,
                    &fixture.playlist,
                    PlaylistDownloadQuery::default(),
                )
                .await
                .unwrap();
            if state.desired_revision == state.reconciled_revision
                && state.entries.items.len() == count
                && state
                    .entries
                    .items
                    .iter()
                    .all(|entry| entry.status == expected)
            {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("manual workers must reconcile the authoritative Follow membership")
}

fn owned_audio(fixture: &Fixture) -> Vec<Vec<u8>> {
    std::fs::read_dir(fixture.directory.path().join("hito.downloads"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            let name = entry.file_name().into_string().unwrap();
            let parts: Vec<&str> = name.split('.').collect();
            assert_eq!(parts.len(), 3);
            assert_eq!(parts[2], "media");
            for part in &parts[..2] {
                assert!(Uuid::parse_str(part).is_ok());
            }
            std::fs::read(entry.path()).unwrap()
        })
        .collect()
}

async fn audio(fixture: &Fixture, source: &str) -> axum::response::Response {
    fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/sources/{source}/audio"))
                .header("authorization", format!("Bearer {}", fixture.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

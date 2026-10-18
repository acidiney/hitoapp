#[path = "fixtures/playlist_download_availability_fixture.rs"]
mod fixture;
#[path = "api/support.rs"]
#[allow(dead_code)]
mod http;
#[path = "../../hito-core/tests/fixtures/playlist_service.rs"]
mod provider;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use fixture::Fixture;
use hito_core::{
    DownloadRequest, PlaylistDownloadEntryState, PlaylistDownloadQuery, PlaylistDownloadStatus,
    RefreshFollowedPlaylist,
};
use http_body_util::BodyExt;
use std::{future::pending, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn a_downloaded_follow_fetches_an_entry_that_becomes_available_under_its_stable_uuid() {
    let fixture = Fixture::start().await;
    let detail = fixture
        .services
        .collection
        .playlist(&fixture.actor, &fixture.playlist)
        .await
        .unwrap();
    assert_eq!(detail.entries.len(), 1);
    let stable_entry = detail.entries[0].entry_id.clone();
    Uuid::parse_str(&stable_entry).unwrap();
    assert!(detail.entries[0].recording_id.is_none());
    let origin = detail.entries[0].origin.as_ref().unwrap();
    assert_eq!(origin.occurrence_id.as_deref(), Some(fixture::OCCURRENCE));
    assert_eq!(
        origin.provider_recording_id.as_deref(),
        Some(fixture::RECORDING)
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
    let idle = advance_until(&fixture, PlaylistDownloadEntryState::Unavailable).await;
    assert_eq!(idle.retention_id, receipt.retention_id);
    assert_eq!(idle.entries.items.len(), 1);
    assert_eq!(idle.entries.items[0].entry_id, stable_entry);
    assert!(idle.entries.items[0].recording_id.is_none());
    assert_eq!(
        idle.entries.items[0].error.as_deref(),
        Some("unavailable_item")
    );
    assert!(
        !fixture
            .services
            .downloads
            .run_next_playlist_reconciliation_until(pending())
            .await
            .unwrap()
    );
    assert!(
        !fixture
            .services
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    assert!(fixture.provider.requests("v1/audio").await.is_empty());

    fixture::snapshot(&fixture.provider, "same-occurrence-available", true).await;
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
    let detail = fixture
        .services
        .collection
        .playlist(&fixture.actor, &fixture.playlist)
        .await
        .unwrap();
    assert_eq!(
        detail.origin.as_ref().unwrap().snapshot,
        "same-occurrence-available"
    );
    assert_eq!(detail.entries.len(), 1);
    assert_eq!(detail.entries[0].entry_id, stable_entry);
    assert_eq!(
        detail.entries[0].recording_id.as_deref(),
        Some(fixture.recording.as_str())
    );
    assert_eq!(
        detail.entries[0]
            .origin
            .as_ref()
            .unwrap()
            .occurrence_id
            .as_deref(),
        Some(fixture::OCCURRENCE)
    );
    let changed = status(&fixture).await;
    assert!(
        changed.desired_revision > idle.desired_revision,
        "a stable Follow entry acquiring a recording must advance download membership even when entry UUIDs do not change"
    );
    assert!(fixture.provider.requests("v1/audio").await.is_empty());
    let ready = advance_until(&fixture, PlaylistDownloadEntryState::Ready).await;
    assert_eq!(ready.retention_id, receipt.retention_id);
    assert_eq!(ready.entries.items.len(), 1);
    let entry = &ready.entries.items[0];
    assert_eq!(entry.entry_id, stable_entry);
    assert_eq!(
        entry.recording_id.as_deref(),
        Some(fixture.recording.as_str())
    );
    assert!(entry.error.is_none());
    assert!(
        matches!(
            fixture
                .services
                .downloads
                .status(&fixture.actor, &fixture.recording)
                .await,
            Err(hito_core::Error::NotFound)
        ),
        "Follow downloads must retain the playlist root without creating a direct recording root"
    );
    let source = entry.source_id.as_deref().unwrap();
    Uuid::parse_str(source).unwrap();
    let sources = fixture
        .get(&format!("/api/v1/recordings/{}/sources", fixture.recording))
        .await;
    assert_eq!(sources.as_array().unwrap().len(), 2);
    assert!(
        sources
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == source && item["provider"] == "download")
    );
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
    assert!(
        fixture.provider.requests("v1/audio").await[0].1
            == serde_json::json!({"source_id": fixture::SOURCE, "_method": "GET"})
    );
    fixture.provider.offline().await;
    let response = fixture
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
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    let audio = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(audio.as_ref(), fixture.audio.as_slice());
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
    assert_eq!(fixture.saved_state().await, preserved);
    assert_eq!(
        std::fs::read(fixture.directory.path().join("music/original.wav")).unwrap(),
        fixture.original
    );
    fixture.services.downloads.finish_downloads().await.unwrap();
}

async fn status(fixture: &Fixture) -> PlaylistDownloadStatus {
    fixture
        .services
        .downloads
        .playlist_status(
            &fixture.actor,
            &fixture.playlist,
            PlaylistDownloadQuery::default(),
        )
        .await
        .unwrap()
}

async fn advance_until(
    fixture: &Fixture,
    expected: PlaylistDownloadEntryState,
) -> PlaylistDownloadStatus {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let reconciliation_progressed = fixture
                .services
                .downloads
                .run_next_playlist_reconciliation_until(pending())
                .await
                .unwrap();
            let download_progressed = fixture
                .services
                .downloads
                .run_next_job_until(pending())
                .await
                .unwrap();
            let state = status(fixture).await;
            if !reconciliation_progressed
                && !download_progressed
                && state.desired_revision == state.reconciled_revision
                && state.entries.items.len() == 1
                && state.entries.items[0].status == expected
            {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("manual worker advancement must reconcile the accepted Follow download")
}

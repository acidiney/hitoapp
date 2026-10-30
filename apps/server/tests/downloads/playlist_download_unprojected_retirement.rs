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
    DownloadRequest, DownloadState, Error, PlaylistDownloadQuery, RefreshFollowedPlaylist,
};
use http_body_util::BodyExt;
use std::{future::pending, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn removing_an_unprojected_playlist_reclaims_a_transfer_started_by_its_removed_direct_root() {
    let fixture = Fixture::start().await;
    fixture::snapshot(&fixture.provider, "ready-before-download", true).await;
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
    assert_eq!(detail.entries.len(), 1);
    assert_eq!(
        detail.entries[0].recording_id.as_deref(),
        Some(fixture.recording.as_str())
    );
    let preserved = fixture.saved_state().await;
    assert_eq!(preserved["catalog"]["total"], 2);
    assert_eq!(preserved["copy"]["entries"].as_array().unwrap().len(), 1);
    assert!(preserved["metadata"]["title"]["personal"] == "My private title");
    assert!(preserved["metadata"]["artist"]["personal"] == "My private artist");
    assert!(preserved["activity"]["history"][0]["position_ms"] == 700);
    assert!(preserved["activity"]["history"][0]["speed"] == 1.25);
    let direct_request = Uuid::new_v4();
    let direct = fixture
        .services
        .downloads
        .create(
            &fixture.actor,
            &fixture.recording,
            DownloadRequest {
                request_id: direct_request,
            },
        )
        .await
        .unwrap();
    let queued = fixture
        .services
        .downloads
        .status(&fixture.actor, &fixture.recording)
        .await
        .unwrap();
    assert!(matches!(queued.status, DownloadState::Queued));
    assert_eq!(queued.intent_id, direct.intent_id);
    assert!(owned_audio(&fixture).is_empty());
    assert!(fixture.provider.requests("v1/audio").await.is_empty());
    let playlist_request = Uuid::new_v4();
    let playlist = fixture
        .services
        .downloads
        .create_playlist(
            &fixture.actor,
            &fixture.playlist,
            DownloadRequest {
                request_id: playlist_request,
            },
        )
        .await
        .unwrap();
    assert_ne!(playlist.retention_id, direct.intent_id);
    let unprojected = fixture
        .services
        .downloads
        .playlist_status(
            &fixture.actor,
            &fixture.playlist,
            PlaylistDownloadQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(unprojected.retention_id, playlist.retention_id);
    assert_eq!(unprojected.reconciled_revision, 0);
    assert!(unprojected.desired_revision > 0);
    fixture
        .services
        .downloads
        .remove(&fixture.actor, &direct.intent_id)
        .await
        .unwrap();
    assert!(matches!(
        fixture
            .services
            .downloads
            .status(&fixture.actor, &fixture.recording)
            .await,
        Err(Error::NotFound)
    ));
    assert!(owned_audio(&fixture).is_empty());
    assert!(fixture.provider.requests("v1/audio").await.is_empty());

    assert!(
        fixture
            .services
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap(),
        "the current playlist membership must retain the queued transfer before projection"
    );
    let still_unprojected = fixture
        .services
        .downloads
        .playlist_status(
            &fixture.actor,
            &fixture.playlist,
            PlaylistDownloadQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(still_unprojected.reconciled_revision, 0);
    assert_eq!(
        still_unprojected.desired_revision,
        unprojected.desired_revision
    );
    assert_eq!(still_unprojected.entries.items.len(), 1);
    assert_eq!(
        still_unprojected.entries.items[0].entry_id,
        detail.entries[0].entry_id
    );
    let sources = fixture
        .get(&format!("/api/v1/recordings/{}/sources", fixture.recording))
        .await;
    assert_eq!(sources.as_array().unwrap().len(), 2);
    let managed = sources
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["provider"] == "download")
        .unwrap();
    assert!(managed["ready"] == true);
    let managed_source = managed["id"].as_str().unwrap();
    Uuid::parse_str(managed_source).unwrap();
    let files = owned_audio(&fixture);
    assert_eq!(
        files.len(),
        1,
        "the retained queued transfer must publish one managed copy"
    );
    assert!(
        files[0] == fixture.audio,
        "the owned file must contain verified fixture audio"
    );
    let response = audio(&fixture, managed_source).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.as_ref() == fixture.audio.as_slice());
    assert!(
        matches!(
            fixture
                .services
                .downloads
                .status(&fixture.actor, &fixture.recording)
                .await,
            Err(Error::NotFound)
        ),
        "playlist retention must not recreate the removed direct root"
    );
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
    assert_eq!(fixture.saved_state().await, preserved);

    fixture
        .services
        .downloads
        .remove(&fixture.actor, &playlist.retention_id)
        .await
        .unwrap();
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
    assert_eq!(
        audio(&fixture, managed_source).await.status(),
        StatusCode::NOT_FOUND
    );
    let _ = tokio::time::timeout(Duration::from_secs(8), async {
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
            if owned_audio(&fixture).is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(
        owned_audio(&fixture).is_empty(),
        "removing the last unprojected playlist root must physically reclaim the managed copy"
    );
    let replayed_direct = fixture
        .services
        .downloads
        .create(
            &fixture.actor,
            &fixture.recording,
            DownloadRequest {
                request_id: direct_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(replayed_direct.intent_id, direct.intent_id);
    let replayed_playlist = fixture
        .services
        .downloads
        .create_playlist(
            &fixture.actor,
            &fixture.playlist,
            DownloadRequest {
                request_id: playlist_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(replayed_playlist.retention_id, playlist.retention_id);
    assert!(matches!(
        fixture
            .services
            .downloads
            .status(&fixture.actor, &fixture.recording)
            .await,
        Err(Error::NotFound)
    ));
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
    fixture.services.downloads.finish_downloads().await.unwrap();
    assert!(
        owned_audio(&fixture).is_empty(),
        "receipt replay must leave managed storage empty"
    );
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
    assert_eq!(fixture.saved_state().await, preserved);
    assert!(
        std::fs::read(fixture.directory.path().join("music/original.wav")).unwrap()
            == fixture.original
    );
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

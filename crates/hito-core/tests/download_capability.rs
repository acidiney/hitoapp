#[path = "fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use audio_fixture::AudioLibrary;
use hito_core::{AudioRequest, Credentials, DownloadRequest, DownloadState, Error, OpenedAudio};
use std::{future::pending, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn explicit_download_outlives_its_session_and_old_receipts_never_spawn_after_removal() {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter("off,hito_core::library_storage::managed=debug,hito_core::library_storage::inspection=debug,hito_core::downloads=debug")
        .with_test_writer()
        .finish();
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let session = fixture
        .hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let requesting = fixture
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let request = Uuid::new_v4();
    let accepted = fixture
        .hito
        .downloads
        .create(
            &requesting,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: request,
            },
        )
        .await
        .unwrap();
    assert!(fixture.server.requests("v1/audio").await.is_empty());
    fixture.hito.identity.logout(&session.token).await.unwrap();
    assert!(
        fixture
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    let ready = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(
        matches!(ready.status, DownloadState::Ready),
        "download state={:?}, error={:?}, received={}, total={:?}",
        ready.status,
        ready.error,
        ready.bytes_received,
        ready.total_bytes
    );
    assert_eq!(ready.intent_id, accepted.intent_id);
    assert_eq!(ready.bytes_received, fixture.original.len() as u64);
    assert_eq!(ready.total_bytes, Some(fixture.original.len() as u64));
    assert_eq!(ready.host_device_id, "provider-audio");
    let source = ready.source_id.unwrap();
    assert_ne!(source, fixture.source.id);
    let OpenedAudio::Content(mut audio) = fixture
        .hito
        .catalog
        .open_audio(&fixture.actor, &source, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("verified retained audio must open");
    };
    let mut bytes = Vec::new();
    while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes, fixture.original);
    let transfers = fixture.server.requests("v1/audio").await.len();
    fixture
        .hito
        .downloads
        .remove(&fixture.actor, &accepted.intent_id)
        .await
        .unwrap();
    fixture
        .hito
        .downloads
        .remove(&fixture.actor, &accepted.intent_id)
        .await
        .unwrap();
    assert!(matches!(
        fixture
            .hito
            .downloads
            .status(&fixture.actor, &fixture.source.recording_id)
            .await,
        Err(Error::NotFound)
    ));
    let replay = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: request,
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.intent_id, accepted.intent_id);
    assert!(
        !fixture
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    assert_eq!(fixture.server.requests("v1/audio").await.len(), transfers);
    let fresh = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert_ne!(fresh.intent_id, accepted.intent_id);
    fixture
        .hito
        .downloads
        .remove(&fixture.actor, &fresh.intent_id)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(fixture.music.join("original.wav")).unwrap(),
        fixture.original
    );
}

#[tokio::test]
async fn removing_an_intent_fences_a_pending_provider_body_and_preserves_original_audio() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let gate = fixture.server.hold_broken_tail(64).await;
    let accepted = fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    let downloads = fixture.hito.downloads.clone();
    let worker = tokio::spawn(async move { downloads.run_next_job_until(pending()).await });
    tokio::time::timeout(Duration::from_secs(2), gate.entered())
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        fixture
            .hito
            .downloads
            .remove(&fixture.actor, &accepted.intent_id),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    gate.open();
    assert!(matches!(
        fixture
            .hito
            .downloads
            .status(&fixture.actor, &fixture.source.recording_id)
            .await,
        Err(Error::NotFound)
    ));
    let sources = fixture
        .hito
        .catalog
        .sources(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, fixture.source.id);
    assert_eq!(
        std::fs::read(fixture.music.join("original.wav")).unwrap(),
        fixture.original
    );
}

#[tokio::test]
async fn invalid_original_audio_is_never_published_as_a_local_playback_source() {
    let fixture = AudioLibrary::start().await;
    fixture
        .server
        .audio(
            axum::http::StatusCode::OK,
            audio_fixture::headers(),
            b"private invalid audio fixture".to_vec(),
        )
        .await;
    fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert!(
        fixture
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    let status = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(status.status, DownloadState::Failed));
    assert_eq!(status.error.as_deref(), Some("transfer_failed"));
    assert!(status.source_id.is_none());
    let sources = fixture
        .hito
        .catalog
        .sources(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, fixture.source.id);
    assert_eq!(
        std::fs::read(fixture.music.join("original.wav")).unwrap(),
        fixture.original
    );
}

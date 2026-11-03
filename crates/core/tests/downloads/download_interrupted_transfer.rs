use hito_core::test_support::provider_audio as audio_fixture;

use audio_fixture::AudioLibrary;
use hito_core::{DownloadRequest, DownloadState};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{future::pending, time::Duration};
use uuid::Uuid;

fn managed_names(fixture: &AudioLibrary) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(fixture.directory.path().join("hito.downloads"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

async fn make_due(fixture: &AudioLibrary) {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(fixture.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE download_jobs SET next_attempt_at=0")
        .execute(&mut connection)
        .await
        .unwrap();
}

async fn request(fixture: &AudioLibrary) {
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
}

async fn run(fixture: &AudioLibrary) -> bool {
    tokio::time::timeout(
        Duration::from_secs(20),
        fixture.hito.downloads.run_next_job_until(pending()),
    )
    .await
    .unwrap()
    .unwrap()
}

#[tokio::test]
async fn a_connection_that_drops_mid_body_publishes_nothing_and_the_retry_restarts_cleanly() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let gate = fixture.server.hold_broken_tail(64).await;
    request(&fixture).await;
    let downloads = fixture.hito.downloads.clone();
    let worker = tokio::spawn(async move { downloads.run_next_job_until(pending()).await });
    tokio::time::timeout(Duration::from_secs(10), gate.entered())
        .await
        .unwrap();
    gate.open();
    assert!(
        tokio::time::timeout(Duration::from_secs(20), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    let waiting = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(waiting.status, DownloadState::Queued));
    assert_eq!(waiting.error.as_deref(), Some("provider_unavailable"));
    assert!(waiting.source_id.is_none());
    assert_eq!(managed_names(&fixture), Vec::<String>::new());
    assert_eq!(
        fixture
            .hito
            .catalog
            .sources(&fixture.actor, &fixture.source.recording_id)
            .await
            .unwrap()
            .len(),
        1
    );

    fixture.server.clear_audio_tail_fault().await;
    make_due(&fixture).await;
    assert!(run(&fixture).await);
    let ready = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(ready.status, DownloadState::Ready));
    assert_eq!(ready.bytes_received, fixture.original.len() as u64);
    let names = managed_names(&fixture);
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].ends_with(".media"));
    assert_eq!(fixture.server.requests("v1/audio").await.len(), 2);
}

#[tokio::test]
async fn a_body_that_ends_early_is_never_published_and_a_complete_retry_succeeds() {
    let fixture = AudioLibrary::start().await;
    fixture
        .server
        .audio(
            axum::http::StatusCode::OK,
            audio_fixture::headers(),
            fixture.original[..fixture.original.len() / 2].to_vec(),
        )
        .await;
    request(&fixture).await;
    assert!(run(&fixture).await);
    let failed = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(failed.status, DownloadState::Failed));
    assert_eq!(failed.error.as_deref(), Some("transfer_failed"));
    assert!(failed.source_id.is_none());
    assert_eq!(managed_names(&fixture), Vec::<String>::new());
    assert_eq!(
        fixture
            .hito
            .catalog
            .sources(&fixture.actor, &fixture.source.recording_id)
            .await
            .unwrap()
            .len(),
        1
    );

    fixture.ready().await;
    fixture
        .hito
        .downloads
        .remove(&fixture.actor, &failed.intent_id)
        .await
        .unwrap();
    request(&fixture).await;
    assert!(run(&fixture).await);
    let ready = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(ready.status, DownloadState::Ready));
    let names = managed_names(&fixture);
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].ends_with(".media"));
}

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

#[tokio::test]
async fn a_shutdown_requeue_and_a_provider_retry_are_not_reported_as_missing_disk_space() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let gate = fixture.server.hold_broken_tail(64).await;
    request(&fixture).await;
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let downloads = fixture.hito.downloads.clone();
    let worker = tokio::spawn(async move {
        downloads
            .run_next_job_until(async {
                let _ = stopped.await;
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), gate.entered())
        .await
        .unwrap();
    stop.send(()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(20), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    let requeued = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(requeued.status, DownloadState::Queued));
    assert_eq!(requeued.error, None);
    gate.open();

    assert!(run(&fixture).await);
    let retrying = fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(retrying.status, DownloadState::Queued));
    assert_eq!(retrying.error.as_deref(), Some("provider_unavailable"));
}

async fn state(fixture: &AudioLibrary) -> hito_core::DownloadStatus {
    fixture
        .hito
        .downloads
        .status(&fixture.actor, &fixture.source.recording_id)
        .await
        .unwrap()
}

#[tokio::test]
async fn expired_and_failing_links_retry_three_times_then_fail_with_a_reason_and_stop_asking() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    fixture.server.audio_statuses(vec![
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        axum::http::StatusCode::GONE,
        axum::http::StatusCode::FORBIDDEN,
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
    ]);
    request(&fixture).await;
    for expected_requests in [1, 2] {
        assert!(run(&fixture).await);
        let waiting = state(&fixture).await;
        assert!(matches!(waiting.status, DownloadState::Queued));
        assert_eq!(waiting.error.as_deref(), Some("provider_unavailable"));
        assert_eq!(
            fixture.server.requests("v1/audio").await.len(),
            expected_requests
        );
        make_due(&fixture).await;
    }
    assert!(run(&fixture).await);
    let failed = state(&fixture).await;
    assert!(matches!(failed.status, DownloadState::Failed));
    assert_eq!(failed.error.as_deref(), Some("transfer_failed"));
    assert!(failed.source_id.is_none());
    assert_eq!(managed_names(&fixture), Vec::<String>::new());

    make_due(&fixture).await;
    assert!(
        !run(&fixture).await,
        "a failed download must not be retried by itself"
    );
    assert_eq!(fixture.server.requests("v1/audio").await.len(), 3);

    fixture
        .hito
        .downloads
        .remove(&fixture.actor, &failed.intent_id)
        .await
        .unwrap();
    fixture.server.audio_statuses(Vec::new());
    request(&fixture).await;
    assert!(run(&fixture).await);
    assert!(matches!(state(&fixture).await.status, DownloadState::Ready));
    assert_eq!(managed_names(&fixture).len(), 1);
}

#[tokio::test]
async fn a_short_burst_of_provider_errors_is_retried_and_the_download_completes() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    fixture.server.audio_statuses(vec![
        axum::http::StatusCode::BAD_GATEWAY,
        axum::http::StatusCode::GATEWAY_TIMEOUT,
    ]);
    request(&fixture).await;
    for _ in 0..2 {
        assert!(run(&fixture).await);
        assert_eq!(
            state(&fixture).await.error.as_deref(),
            Some("provider_unavailable")
        );
        make_due(&fixture).await;
    }
    assert!(run(&fixture).await);
    let ready = state(&fixture).await;
    assert!(matches!(ready.status, DownloadState::Ready));
    assert_eq!(ready.error, None);
    assert_eq!(ready.bytes_received, fixture.original.len() as u64);
    assert_eq!(managed_names(&fixture).len(), 1);
    assert_eq!(fixture.server.requests("v1/audio").await.len(), 3);
}

#[tokio::test]
async fn a_silent_provider_requeues_the_download_at_the_header_bound_and_a_later_attempt_completes()
{
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let gate = fixture.server.hold("v1/audio").await;
    request(&fixture).await;
    let started = std::time::Instant::now();
    assert!(run(&fixture).await);
    let waited = started.elapsed();
    assert!(
        (Duration::from_secs(9)..Duration::from_secs(14)).contains(&waited),
        "{waited:?}"
    );
    let waiting = state(&fixture).await;
    assert!(matches!(waiting.status, DownloadState::Queued));
    assert_eq!(waiting.error.as_deref(), Some("provider_unavailable"));
    assert_eq!(managed_names(&fixture), Vec::<String>::new());
    gate.open();
    make_due(&fixture).await;
    assert!(run(&fixture).await);
    assert!(matches!(state(&fixture).await.status, DownloadState::Ready));
}

#[tokio::test]
async fn a_slowly_trickling_provider_completes_the_download_with_every_byte() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    fixture
        .server
        .audio_trickle(fixture.original.len() / 6 + 1, Duration::from_millis(900));
    request(&fixture).await;
    assert!(run(&fixture).await);
    let ready = state(&fixture).await;
    assert!(matches!(ready.status, DownloadState::Ready));
    assert_eq!(ready.bytes_received, fixture.original.len() as u64);
    assert_eq!(managed_names(&fixture).len(), 1);
}

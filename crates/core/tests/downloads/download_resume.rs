use super::download_interrupted_transfer::{make_due, managed_names, request, run, state};
use axum::http::{HeaderMap, StatusCode};
use hito_core::test_support::metadata_tag_logs::CapturedLogs;
use hito_core::{
    DownloadState,
    test_support::provider_audio::{self as audio_fixture, AudioLibrary},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{future::pending, time::Duration};
use tracing::instrument::WithSubscriber;

const PREFIX: usize = 6000;

fn validated(etag: Option<&str>) -> HeaderMap {
    let mut headers = audio_fixture::headers();
    if let Some(etag) = etag {
        headers.insert("etag", etag.parse().unwrap());
    }
    headers
}

async fn serving(fixture: &AudioLibrary, etag: Option<&str>) {
    fixture
        .server
        .audio(StatusCode::OK, validated(etag), fixture.original.clone())
        .await;
    fixture.server.range_chunks(usize::MAX / 2).await;
}

async fn interrupted_download(
    fixture: &AudioLibrary,
    before_reset: impl std::future::Future<Output = ()>,
) {
    let gate = fixture.server.hold_broken_tail(PREFIX).await;
    request(fixture).await;
    let downloads = fixture.hito.downloads.clone();
    let worker = tokio::spawn(
        async move { downloads.run_next_job_until(pending()).await }.with_current_subscriber(),
    );
    tokio::time::timeout(Duration::from_secs(10), gate.entered())
        .await
        .unwrap();
    before_reset.await;
    fixture.server.clear_audio_tail_fault().await;
    assert!(
        tokio::time::timeout(Duration::from_secs(20), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
}

async fn database(fixture: &AudioLibrary) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(fixture.directory.path().join("hito.db")),
    )
    .await
    .unwrap()
}

async fn stored_checksum(fixture: &AudioLibrary) -> String {
    sqlx::query_scalar("SELECT checksum FROM download_files WHERE state='ready'")
        .fetch_one(&mut database(fixture).await)
        .await
        .unwrap()
}

async fn attempts(fixture: &AudioLibrary) -> i64 {
    sqlx::query_scalar("SELECT attempts FROM download_jobs")
        .fetch_one(&mut database(fixture).await)
        .await
        .unwrap()
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

async fn range_headers(fixture: &AudioLibrary) -> Vec<Option<String>> {
    fixture
        .server
        .requests("v1/audio")
        .await
        .into_iter()
        .map(|(headers, _)| {
            headers
                .get("range")
                .map(|value| value.to_str().unwrap().to_owned())
        })
        .collect()
}

#[tokio::test]
async fn an_interrupted_transfer_continues_from_the_received_bytes_in_the_same_attempt() {
    let fixture = AudioLibrary::start().await;
    serving(&fixture, Some("\"v1\"")).await;
    interrupted_download(&fixture, async {}).await;

    let ready = state(&fixture).await;
    assert!(matches!(ready.status, DownloadState::Ready));
    assert_eq!(ready.bytes_received, fixture.original.len() as u64);
    assert_eq!(
        range_headers(&fixture).await,
        vec![None, Some(format!("bytes={PREFIX}-"))]
    );
    let fetched = PREFIX + (fixture.original.len() - PREFIX);
    assert!((fixture.original.len()..=fixture.original.len() + 4096).contains(&fetched));
    assert_eq!(stored_checksum(&fixture).await, digest(&fixture.original));
    assert_eq!(attempts(&fixture).await, 1);
    assert_eq!(managed_names(&fixture).len(), 1);
}

#[tokio::test]
async fn a_provider_without_validators_resumes_on_the_declared_length_alone() {
    let fixture = AudioLibrary::start().await;
    serving(&fixture, None).await;
    interrupted_download(&fixture, async {}).await;

    assert!(matches!(state(&fixture).await.status, DownloadState::Ready));
    assert_eq!(
        range_headers(&fixture).await,
        vec![None, Some(format!("bytes={PREFIX}-"))]
    );
    assert_eq!(stored_checksum(&fixture).await, digest(&fixture.original));
}

#[tokio::test]
async fn a_provider_that_ignores_the_resume_range_restarts_from_the_first_byte() {
    let fixture = AudioLibrary::start().await;
    serving(&fixture, Some("\"v1\"")).await;
    fixture.server.audio_ignores_ranges();
    interrupted_download(&fixture, async {}).await;

    let ready = state(&fixture).await;
    assert!(matches!(ready.status, DownloadState::Ready));
    assert_eq!(ready.bytes_received, fixture.original.len() as u64);
    assert_eq!(
        range_headers(&fixture).await,
        vec![None, Some(format!("bytes={PREFIX}-"))]
    );
    assert_eq!(stored_checksum(&fixture).await, digest(&fixture.original));
    assert_eq!(attempts(&fixture).await, 1);
}

#[tokio::test]
async fn a_changed_entity_tag_restarts_from_the_first_byte_and_never_splices_two_versions() {
    let fixture = AudioLibrary::start().await;
    serving(&fixture, Some("\"v1\"")).await;
    let mut replacement = fixture.original.clone();
    for byte in &mut replacement[44..] {
        *byte ^= 0x55;
    }
    assert_ne!(replacement, fixture.original);
    interrupted_download(&fixture, async {
        fixture
            .server
            .audio(
                StatusCode::OK,
                validated(Some("\"v2\"")),
                replacement.clone(),
            )
            .await;
    })
    .await;

    let ready = state(&fixture).await;
    assert!(matches!(ready.status, DownloadState::Ready));
    assert_eq!(ready.bytes_received, replacement.len() as u64);
    assert_eq!(
        range_headers(&fixture).await,
        vec![None, Some(format!("bytes={PREFIX}-")), None]
    );
    assert_eq!(stored_checksum(&fixture).await, digest(&replacement));
    assert_eq!(attempts(&fixture).await, 1);
    assert_eq!(managed_names(&fixture).len(), 1);
}

#[tokio::test]
async fn a_failed_resume_spends_the_attempt_like_any_failure_and_the_retry_starts_clean() {
    let fixture = AudioLibrary::start().await;
    serving(&fixture, Some("\"v1\"")).await;
    interrupted_download(&fixture, async {
        fixture
            .server
            .audio_statuses(vec![StatusCode::SERVICE_UNAVAILABLE]);
    })
    .await;

    let waiting = state(&fixture).await;
    assert!(matches!(waiting.status, DownloadState::Queued));
    assert_eq!(waiting.error.as_deref(), Some("provider_unavailable"));
    assert_eq!(managed_names(&fixture), Vec::<String>::new());
    assert_eq!(attempts(&fixture).await, 1);

    make_due(&fixture).await;
    assert!(run(&fixture).await);
    assert!(matches!(state(&fixture).await.status, DownloadState::Ready));
    assert_eq!(
        range_headers(&fixture).await,
        vec![None, Some(format!("bytes={PREFIX}-")), None]
    );
    assert_eq!(stored_checksum(&fixture).await, digest(&fixture.original));
    assert_eq!(attempts(&fixture).await, 2);
}

#[tokio::test]
async fn a_resumed_transfer_logs_its_outcome_and_offset_and_nothing_private() {
    let logs = CapturedLogs::default();
    let url = async {
        let fixture = AudioLibrary::start().await;
        serving(&fixture, Some("\"tag-9f3a\"")).await;
        interrupted_download(&fixture, async {}).await;
        assert!(matches!(state(&fixture).await.status, DownloadState::Ready));
        fixture.server.url.clone()
    }
    .with_subscriber(logs.subscriber())
    .await;
    let output = logs.text();
    let resumed: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|event| event["fields"]["event"] == "download_transfer_resumed")
        .collect();
    assert_eq!(resumed.len(), 1, "{output}");
    assert_eq!(resumed[0]["fields"]["outcome"], "continued");
    assert_eq!(resumed[0]["fields"]["offset"], PREFIX);
    assert!(!output.contains("private-audio-token"));
    assert!(!output.contains(&url));
    assert!(!output.contains("tag-9f3a"));
}

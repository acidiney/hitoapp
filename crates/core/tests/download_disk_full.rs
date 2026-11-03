#![cfg(target_os = "linux")]

#[path = "fixtures/write_fault_shim.rs"]
mod shim;

use hito_core::test_support::metadata_tag_logs::CapturedLogs;
use hito_core::test_support::provider_audio as audio_fixture;

use audio_fixture::AudioLibrary;
use hito_core::{
    AdoptProviderRecording, AudioRequest, DownloadRequest, DownloadState, DownloadStatus,
    OpenedAudio,
};
use shim::Faults;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{future::pending, path::PathBuf, time::Duration};
use uuid::Uuid;

const STORAGE_FULL: &str = "insufficient_storage";

fn capture_logs() -> CapturedLogs {
    let logs = CapturedLogs::default();
    tracing::subscriber::set_global_default(logs.subscriber()).unwrap();
    logs
}

fn faults() -> Faults {
    shim::child_faults().expect("Fault injection runs in the child process")
}

fn managed_names(fixture: &AudioLibrary) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(fixture.directory.path().join("hito.downloads"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn extension_count(fixture: &AudioLibrary, extension: &str) -> usize {
    managed_names(fixture)
        .iter()
        .filter(|name| name.ends_with(extension))
        .count()
}

async fn staged_prefix(fixture: &AudioLibrary, length: u64) {
    let directory = fixture.directory.path().join("hito.downloads");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let staged = std::fs::read_dir(&directory).unwrap().any(|entry| {
                let entry = entry.unwrap();
                entry.file_name().to_string_lossy().ends_with(".part")
                    && entry.metadata().unwrap().len() == length
            });
            if staged {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The staged prefix was not written");
}

async fn make_due(fixture: &AudioLibrary) {
    let database: PathBuf = fixture.directory.path().join("hito.db");
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(database))
            .await
            .unwrap();
    sqlx::query("UPDATE download_jobs SET next_attempt_at=0")
        .execute(&mut connection)
        .await
        .unwrap();
}

async fn request(fixture: &AudioLibrary, recording: &str) {
    fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            recording,
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

async fn status(fixture: &AudioLibrary, recording: &str) -> DownloadStatus {
    fixture
        .hito
        .downloads
        .status(&fixture.actor, recording)
        .await
        .unwrap()
}

async fn second_recording(fixture: &AudioLibrary) -> String {
    for (path, first, second) in [
        (
            "v1/recordings/detail",
            serde_json::json!({"id":"recording","title":"Fixture track","artist":"Fixture artist"}),
            serde_json::json!({"id":"second","title":"Second track","artist":"Fixture artist"}),
        ),
        (
            "v1/sources",
            serde_json::json!({"items":[{"id":"audio:/1?private=value&other=bad","label":"Original","ready":true}]}),
            serde_json::json!({"items":[{"id":"audio:/2","label":"Original","ready":true}]}),
        ),
    ] {
        fixture
            .server
            .recording_reply(path, "recording", first)
            .await;
        fixture.server.recording_reply(path, "second", second).await;
    }
    fixture
        .hito
        .catalog
        .adopt_provider(
            &fixture.actor,
            &fixture.installation.id,
            AdoptProviderRecording {
                installation_revision: 1,
                provider_recording_id: "second".into(),
            },
        )
        .await
        .unwrap()
        .id
}

async fn play(fixture: &AudioLibrary, source: &str) -> Vec<u8> {
    let OpenedAudio::Content(mut audio) = fixture
        .hito
        .playback
        .open_audio(&fixture.actor, source, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("The whole file must be satisfiable");
    };
    let mut bytes = Vec::new();
    while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    bytes
}

async fn download_source(fixture: &AudioLibrary, recording: &str) -> String {
    let ready = status(fixture, recording).await;
    assert!(matches!(ready.status, DownloadState::Ready));
    ready.source_id.unwrap()
}

#[tokio::test]
async fn a_full_disk_mid_download_queues_a_storage_state_and_publishes_nothing() {
    if shim::child_faults().is_none() {
        shim::run_in_child("a_full_disk_mid_download_queues_a_storage_state_and_publishes_nothing")
            .await;
        return;
    }
    let logs = capture_logs();
    let faults = faults();
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let tail = fixture.server.hold_audio_tail(64);
    let recording = fixture.source.recording_id.clone();
    request(&fixture, &recording).await;
    let downloads = fixture.hito.downloads.clone();
    let worker = tokio::spawn(async move { downloads.run_next_job_until(pending()).await });
    tokio::time::timeout(Duration::from_secs(10), tail.entered())
        .await
        .unwrap();
    staged_prefix(&fixture, 64).await;
    assert_eq!(extension_count(&fixture, ".part"), 1);
    faults.set("full-disk");
    tail.release();
    assert!(
        tokio::time::timeout(Duration::from_secs(20), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    let waiting = status(&fixture, &recording).await;
    assert!(
        matches!(waiting.status, DownloadState::Queued),
        "{:?} {:?}",
        waiting.status,
        waiting.error
    );
    assert_eq!(waiting.error.as_deref(), Some(STORAGE_FULL));
    assert!(waiting.source_id.is_none());
    assert_eq!(managed_names(&fixture), Vec::<String>::new());
    let sources = fixture
        .hito
        .catalog
        .sources(&fixture.actor, &recording)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, fixture.source.id);
    assert!(!run(&fixture).await);
    assert_eq!(fixture.server.requests("v1/audio").await.len(), 1);

    faults.clear("full-disk");
    make_due(&fixture).await;
    assert!(run(&fixture).await);
    let source = download_source(&fixture, &recording).await;
    assert_eq!(play(&fixture, &source).await, fixture.original);
    assert_eq!(extension_count(&fixture, ".part"), 0);
    assert_eq!(extension_count(&fixture, ".media"), 1);

    let text = logs.text();
    assert!(text.contains(STORAGE_FULL));
    for private in [
        fixture.directory.path().to_str().unwrap(),
        "hito.downloads",
        ".part",
        "private-audio-token",
        "Fixture track",
    ] {
        assert!(!text.contains(private), "logs leaked {private}");
    }
}

#[tokio::test]
async fn a_full_disk_backs_off_every_queued_download_and_the_next_attempt_succeeds() {
    if shim::child_faults().is_none() {
        shim::run_in_child(
            "a_full_disk_backs_off_every_queued_download_and_the_next_attempt_succeeds",
        )
        .await;
        return;
    }
    let faults = faults();
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let first = fixture.source.recording_id.clone();
    let second = second_recording(&fixture).await;
    faults.set("full-disk");
    request(&fixture, &first).await;
    request(&fixture, &second).await;
    assert!(run(&fixture).await);
    assert!(run(&fixture).await);
    for recording in [&first, &second] {
        let waiting = status(&fixture, recording).await;
        assert!(matches!(waiting.status, DownloadState::Queued));
        assert_eq!(waiting.error.as_deref(), Some(STORAGE_FULL));
    }
    for _ in 0..3 {
        assert!(!run(&fixture).await);
    }
    assert_eq!(fixture.server.requests("v1/audio").await.len(), 2);
    assert_eq!(managed_names(&fixture), Vec::<String>::new());

    faults.clear("full-disk");
    make_due(&fixture).await;
    assert!(run(&fixture).await);
    assert!(run(&fixture).await);
    for recording in [&first, &second] {
        let source = download_source(&fixture, recording).await;
        assert_eq!(play(&fixture, &source).await, fixture.original);
    }
    assert_eq!(fixture.server.requests("v1/audio").await.len(), 4);
    assert_eq!(extension_count(&fixture, ".media"), 2);
    assert_eq!(extension_count(&fixture, ".part"), 0);
}

#[tokio::test]
async fn a_published_download_still_plays_while_the_disk_is_full() {
    if shim::child_faults().is_none() {
        shim::run_in_child("a_published_download_still_plays_while_the_disk_is_full").await;
        return;
    }
    let faults = faults();
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let first = fixture.source.recording_id.clone();
    let second = second_recording(&fixture).await;
    request(&fixture, &first).await;
    assert!(run(&fixture).await);
    let saved = download_source(&fixture, &first).await;

    faults.set("full-disk");
    request(&fixture, &second).await;
    assert!(run(&fixture).await);
    assert_eq!(
        status(&fixture, &second).await.error.as_deref(),
        Some(STORAGE_FULL)
    );
    fixture.server.offline().await;
    assert_eq!(play(&fixture, &saved).await, fixture.original);
    assert!(matches!(
        status(&fixture, &first).await.status,
        DownloadState::Ready
    ));
    assert_eq!(extension_count(&fixture, ".media"), 1);
    assert_eq!(extension_count(&fixture, ".part"), 0);
}

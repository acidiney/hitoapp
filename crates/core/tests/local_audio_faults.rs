#![cfg(target_os = "linux")]

#[path = "fixtures/read_fault_shim.rs"]
mod shim;

use hito_core::test_support::hang_guard::hang_guard;
use hito_core::test_support::metadata_tag_logs::CapturedLogs;
use hito_core::{
    Actor, AudioRequest, ConnectFolder, CoreConfig, Credentials, DirectoryRef, Error, HostDeviceId,
    OpenedAudio, Search, Services, SharedDirectoryConfig,
};
use shim::Faults;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

const TRACKS: [&str; 3] = ["one", "two", "three"];

struct Drives {
    directory: tempfile::TempDir,
    services: Services,
    actor: Actor,
    faults: Faults,
    originals: BTreeMap<String, Vec<u8>>,
    folders: [String; 3],
}

impl Drives {
    fn faulty_root(&self) -> PathBuf {
        self.directory.path().join("hito-faulty-drive")
    }

    fn unplugged_root(&self) -> PathBuf {
        self.directory.path().join("hito-faulty-unplugged")
    }

    async fn source(&self, title: &str) -> String {
        let recording = self.recording(title).await;
        self.services
            .catalog
            .sources(&self.actor, &recording)
            .await
            .unwrap()
            .remove(0)
            .id
    }

    async fn recording(&self, title: &str) -> String {
        self.services
            .catalog
            .recordings(&self.actor, Search::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .find(|recording| recording.title == title)
            .unwrap()
            .id
    }

    async fn available(&self) -> BTreeMap<String, bool> {
        self.services
            .catalog
            .recordings(&self.actor, Search::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|recording| (recording.title, recording.available))
            .collect()
    }

    async fn open(&self, source: &str) -> Result<hito_core::AudioStream, Error> {
        match self
            .services
            .playback
            .open_audio(&self.actor, source, AudioRequest::default())
            .await?
        {
            OpenedAudio::Content(mut content) => Ok(content.body.take().unwrap()),
            OpenedAudio::Unsatisfiable { .. } => panic!("The whole file must be satisfiable"),
        }
    }

    async fn play(&self, source: &str) -> Result<Vec<u8>, Error> {
        let mut stream = self.open(source).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.chunk().await? {
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    async fn rescan(&self, folder: &str) -> String {
        let job = self
            .services
            .storage
            .enqueue_folder_scan(&self.actor, folder)
            .await
            .unwrap();
        assert!(
            hang_guard("scan job", self.services.storage.run_next_job())
                .await
                .unwrap()
        );
        self.services
            .storage
            .job(&self.actor, &job.id)
            .await
            .unwrap()
            .state
    }
}

async fn drives() -> Drives {
    let directory = tempfile::tempdir().unwrap();
    let faulty = directory.path().join("hito-faulty-drive");
    let healthy = directory.path().join("healthy");
    let second = directory.path().join("hito-faulty-second");
    std::fs::create_dir(&second).unwrap();
    std::fs::create_dir(&faulty).unwrap();
    std::fs::create_dir(&healthy).unwrap();
    for folder in [&faulty, &healthy, &second] {
        std::fs::write(
            folder.join("cover.png"),
            hito_core::test_support::covers::COVER_A,
        )
        .unwrap();
    }
    let mut originals = BTreeMap::new();
    for (offset, title) in TRACKS.iter().enumerate() {
        originals.insert(
            title.to_string(),
            write_long_wav(&faulty.join(format!("{title}.wav")), offset as i16),
        );
    }
    for (offset, title) in ["four", "five"].iter().enumerate() {
        originals.insert(
            title.to_string(),
            write_long_wav(&second.join(format!("{title}.wav")), 20 + offset as i16),
        );
    }
    originals.insert(
        "healthy".into(),
        write_long_wav(&healthy.join("healthy.wav"), 7),
    );
    let mut config = CoreConfig::with_shared_directories(
        directory.path().join("hito.db"),
        HostDeviceId::new("server").unwrap(),
        vec![
            SharedDirectoryConfig {
                id: "faulty".into(),
                name: "Faulty".into(),
                root: faulty,
            },
            SharedDirectoryConfig {
                id: "second".into(),
                name: "Second".into(),
                root: second,
            },
            SharedDirectoryConfig {
                id: "healthy".into(),
                name: "Healthy".into(),
                root: healthy,
            },
        ],
    );
    config.media_tools.timeout = Duration::from_secs(2);
    let services = hang_guard("opening the library", hito_core::open(config))
        .await
        .unwrap();
    let session = services
        .identity
        .setup(Credentials {
            username: "admin".into(),
            password: "local audio fault password".into(),
        })
        .await
        .unwrap();
    let actor = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let library = services
        .catalog
        .libraries(&actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    let mut folders = Vec::new();
    for id in ["faulty", "healthy", "second"] {
        let folder = services
            .storage
            .connect_folder(
                &actor,
                &library,
                ConnectFolder {
                    directory: DirectoryRef {
                        directory_id: id.into(),
                        components: Vec::new(),
                    },
                    name: None,
                },
            )
            .await
            .unwrap();
        let job = folder.latest_job.unwrap().id;
        assert!(
            hang_guard("initial scan", services.storage.run_next_job())
                .await
                .unwrap()
        );
        assert_eq!(
            services.storage.job(&actor, &job).await.unwrap().state,
            "complete"
        );
        folders.push(folder.id);
    }
    Drives {
        directory,
        services,
        actor,
        faults: shim::child_faults().expect("Fault injection runs in the child process"),
        originals,
        folders: folders.try_into().unwrap(),
    }
}

fn write_long_wav(path: &Path, offset: i16) -> Vec<u8> {
    let samples = 100_000u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8000u32.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(samples * 2).to_le_bytes());
    for index in 0..samples {
        let sample = ((index % 200) as i16 - 100) * 100 + offset;
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, &bytes).unwrap();
    bytes
}

fn open_handles_on_the_faulty_drive() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| std::fs::read_link(entry.ok()?.path()).ok())
        .filter(|target| target.to_string_lossy().contains("hito-faulty"))
        .count()
}

fn assert_private(logs: &CapturedLogs) {
    let text = logs.text();
    assert!(text.contains("Playback source authorized"), "{text}");
    for private in ["hito-faulty", "one.wav", "healthy.wav", "tmp"] {
        assert!(
            !text.contains(private),
            "{private} leaked into logs: {text}"
        );
    }
}

fn capture_logs() -> CapturedLogs {
    let logs = CapturedLogs::default();
    tracing::subscriber::set_global_default(logs.subscriber()).unwrap();
    logs
}

#[tokio::test]
async fn a_stalled_drive_bounds_the_open_of_local_audio_and_recovers_when_it_returns() {
    if shim::child_faults().is_none() {
        shim::run_in_child(
            "a_stalled_drive_bounds_the_open_of_local_audio_and_recovers_when_it_returns",
        )
        .await;
        return;
    }
    let logs = capture_logs();
    let drives = drives().await;
    let stalled = drives.source("one").await;
    let neighbour = drives.source("two").await;
    let healthy = drives.source("healthy").await;
    drives.faults.set("stall-open");
    let drives = std::sync::Arc::new(drives);
    let mut requests = tokio::task::JoinSet::new();
    for source in [&stalled, &neighbour, &stalled] {
        let drives = drives.clone();
        let source = source.clone();
        requests.spawn(async move { drives.open(&source).await.err() });
    }
    assert_eq!(
        hang_guard("healthy playback", drives.play(&healthy))
            .await
            .unwrap(),
        drives.originals["healthy"]
    );
    while let Some(result) = hang_guard("stalled open", requests.join_next()).await {
        assert!(matches!(result.unwrap(), Some(Error::NotFound)));
    }
    drives.faults.clear("stall-open");
    hang_guard("recovery", async {
        loop {
            if let Ok(bytes) = drives.play(&stalled).await {
                assert_eq!(bytes, drives.originals["one"]);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert_eq!(
        drives.play(&neighbour).await.unwrap(),
        drives.originals["two"]
    );
    assert_eq!(open_handles_on_the_faulty_drive(), 0);
    assert_private(&logs);
}

#[tokio::test]
async fn a_read_that_fails_after_playback_started_ends_the_stream_and_the_drive_can_return() {
    if shim::child_faults().is_none() {
        shim::run_in_child(
            "a_read_that_fails_after_playback_started_ends_the_stream_and_the_drive_can_return",
        )
        .await;
        return;
    }
    let logs = capture_logs();
    let drives = drives().await;
    let source = drives.source("one").await;
    let healthy = drives.source("healthy").await;
    let mut stream = drives.open(&source).await.unwrap();
    let first = stream.chunk().await.unwrap().unwrap();
    assert_eq!(first[..], drives.originals["one"][..first.len()]);
    drives.faults.set("fail-read");
    let error = hang_guard("failing read", stream.chunk())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Internal(_)), "{error:?}");
    assert!(
        hang_guard("ended stream", stream.chunk())
            .await
            .unwrap()
            .is_none()
    );
    drop(stream);
    assert_eq!(open_handles_on_the_faulty_drive(), 0);

    std::fs::rename(drives.faulty_root(), drives.unplugged_root()).unwrap();
    assert!(matches!(
        hang_guard("unplugged open", drives.open(&source))
            .await
            .err(),
        Some(Error::NotFound)
    ));
    assert_eq!(
        drives.play(&healthy).await.unwrap(),
        drives.originals["healthy"]
    );
    std::fs::rename(drives.unplugged_root(), drives.faulty_root()).unwrap();
    drives.faults.clear("fail-read");
    assert_eq!(
        hang_guard("recovered playback", drives.play(&source))
            .await
            .unwrap(),
        drives.originals["one"]
    );
    assert_eq!(open_handles_on_the_faulty_drive(), 0);
    assert_private(&logs);
}

#[tokio::test]
async fn a_read_that_stalls_after_playback_started_times_out_and_playback_resumes_later() {
    if shim::child_faults().is_none() {
        shim::run_in_child(
            "a_read_that_stalls_after_playback_started_times_out_and_playback_resumes_later",
        )
        .await;
        return;
    }
    let logs = capture_logs();
    let drives = drives().await;
    let source = drives.source("one").await;
    let healthy = drives.source("healthy").await;
    let mut stream = drives.open(&source).await.unwrap();
    assert!(stream.chunk().await.unwrap().is_some());
    drives.faults.set("stall-read");
    let error = hang_guard("stalled read", stream.chunk())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Upstream(_)), "{error:?}");
    drop(stream);
    assert_eq!(
        hang_guard("healthy playback", drives.play(&healthy))
            .await
            .unwrap(),
        drives.originals["healthy"]
    );
    drives.faults.clear("stall-read");
    hang_guard("recovery", async {
        loop {
            if open_handles_on_the_faulty_drive() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert_eq!(drives.play(&source).await.unwrap(), drives.originals["one"]);
    assert_private(&logs);
}

#[tokio::test]
async fn a_stalled_drive_bounds_artwork_reads_and_artwork_recovers_when_it_returns() {
    if shim::child_faults().is_none() {
        shim::run_in_child(
            "a_stalled_drive_bounds_artwork_reads_and_artwork_recovers_when_it_returns",
        )
        .await;
        return;
    }
    let drives = std::sync::Arc::new(drives().await);
    let cover = hito_core::test_support::covers::COVER_A;
    let stalled = drives.recording("one").await;
    let neighbour = drives.recording("two").await;
    let healthy = drives.recording("healthy").await;
    drives.faults.set("stall-read");
    let mut requests = tokio::task::JoinSet::new();
    for recording in [
        &stalled, &neighbour, &stalled, &neighbour, &stalled, &neighbour,
    ] {
        let drives = drives.clone();
        let recording = recording.clone();
        requests.spawn(async move {
            drives
                .services
                .catalog
                .artwork(&drives.actor, &recording)
                .await
        });
    }
    let artwork = hang_guard(
        "healthy artwork",
        drives.services.catalog.artwork(&drives.actor, &healthy),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(artwork.bytes, cover);
    while let Some(result) = hang_guard("stalled artwork", requests.join_next()).await {
        assert!(result.unwrap().unwrap().is_none());
    }
    assert_eq!(
        hang_guard(
            "healthy artwork again",
            drives.services.catalog.artwork(&drives.actor, &healthy)
        )
        .await
        .unwrap()
        .unwrap()
        .bytes,
        cover
    );
    drives.faults.clear("stall-read");
    hang_guard("recovery", async {
        loop {
            let found = drives
                .services
                .catalog
                .artwork(&drives.actor, &stalled)
                .await
                .unwrap();
            if let Some(found) = found {
                assert_eq!(found.bytes, cover);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
}

#[tokio::test]
async fn stalled_reads_during_a_scan_skip_the_files_and_do_not_block_healthy_folders() {
    if shim::child_faults().is_none() {
        shim::run_in_child(
            "stalled_reads_during_a_scan_skip_the_files_and_do_not_block_healthy_folders",
        )
        .await;
        return;
    }
    let drives = drives().await;
    let before = drives.available().await;
    assert!(before.values().all(|available| *available), "{before:?}");
    let sources = [
        drives.source("one").await,
        drives.source("two").await,
        drives.source("three").await,
    ];
    drives.faults.set("stall-read");
    assert_eq!(drives.rescan(&drives.folders[0]).await, "complete");
    assert!(open_handles_on_the_faulty_drive() >= 2);
    assert_eq!(drives.rescan(&drives.folders[1]).await, "complete");
    let during = drives.available().await;
    assert!(during["healthy"], "{during:?}");
    for title in TRACKS {
        assert!(!during[title], "{during:?}");
    }
    drives.faults.clear("stall-read");
    assert_eq!(drives.rescan(&drives.folders[0]).await, "complete");
    let after = drives.available().await;
    assert!(after.values().all(|available| *available), "{after:?}");
    for (title, source) in TRACKS.iter().zip(&sources) {
        assert_eq!(&drives.source(title).await, source);
        assert_eq!(drives.play(source).await.unwrap(), drives.originals[*title]);
    }
}

#[tokio::test]
async fn stalled_reads_on_two_drives_do_not_block_a_healthy_folder() {
    if shim::child_faults().is_none() {
        shim::run_in_child("stalled_reads_on_two_drives_do_not_block_a_healthy_folder").await;
        return;
    }
    let drives = drives().await;
    drives.faults.set("stall-read");
    assert_eq!(drives.rescan(&drives.folders[0]).await, "complete");
    assert_eq!(drives.rescan(&drives.folders[2]).await, "complete");
    assert!(open_handles_on_the_faulty_drive() >= 2);
    assert_eq!(drives.rescan(&drives.folders[1]).await, "complete");
    let during = drives.available().await;
    assert!(during["healthy"], "{during:?}");
    for title in ["one", "two", "three", "four", "five"] {
        assert!(!during[title], "{during:?}");
    }
    drives.faults.clear("stall-read");
    assert_eq!(drives.rescan(&drives.folders[0]).await, "complete");
    assert_eq!(drives.rescan(&drives.folders[2]).await, "complete");
    let after = drives.available().await;
    assert!(after.values().all(|available| *available), "{after:?}");
}

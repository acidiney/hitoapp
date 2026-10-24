#![cfg(unix)]
use hito_core::test_support::{media_tool, support};
use hito_core::{
    Actor, ConnectFolder, CoreConfig, Credentials, DirectoryRef, FolderWatchSettings,
    HostDeviceId, Search, Services, WatchBackend, WatchMode, WatchStatus,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};

const WAIT: Duration = Duration::from_secs(20);

struct Harness {
    directory: tempfile::TempDir,
    services: Services,
    actor: Actor,
    library: String,
    music: PathBuf,
    folder: String,
    stop: watch::Sender<bool>,
    watching: Option<JoinHandle<()>>,
    worker: JoinHandle<()>,
    status: watch::Receiver<WatchStatus>,
    jobs_run: Arc<AtomicUsize>,
}

fn settings(mode: WatchMode, debounce_ms: u64) -> FolderWatchSettings {
    FolderWatchSettings {
        mode,
        poll_interval: Duration::from_millis(100),
        debounce: Duration::from_millis(debounce_ms),
    }
}

async fn start(mode: WatchMode, debounce_ms: u64, hold_inspection: bool) -> Harness {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    support::write_wav(&music.join("first.wav"), 0);
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new("watch-test").unwrap(),
    );
    config.folder_watching = settings(mode, debounce_ms);
    if hold_inspection {
        let tools = directory.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        let hold = media_tool::quote(&directory.path().join("hold"));
        config.media_tools.ffprobe = media_tool::script(
            &tools,
            "ffprobe",
            &format!(
                "while [ -e {hold} ]; do sleep 0.05; done\nexec /usr/bin/ffprobe \"$@\""
            ),
        );
    }
    let services = hito_core::open(config).await.unwrap();
    let session = services
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "owned folder watching password".into(),
        })
        .await
        .unwrap();
    let actor = services.identity.authenticate(&session.token).await.unwrap();
    let library = services.catalog.libraries(&actor).await.unwrap().remove(0).id;
    support::scan(&services, &actor, &library).await;
    let folder = services.storage.folders(&actor, &library).await.unwrap().remove(0).id;
    let (stop, stopped) = watch::channel(false);
    let (publish, status) = watch::channel(WatchStatus::new());
    let storage = services.storage.clone();
    let watching = Some(tokio::spawn({
        let stopped = stopped.clone();
        async move { storage.watch_folders(stopped, publish).await }
    }));
    let jobs_run = Arc::new(AtomicUsize::new(0));
    let worker = tokio::spawn({
        let storage = services.storage.clone();
        let jobs_run = jobs_run.clone();
        let mut stopped = stopped;
        async move {
            while !*stopped.borrow() {
                if storage.run_next_job().await.unwrap() {
                    jobs_run.fetch_add(1, Ordering::SeqCst);
                } else {
                    tokio::select! {
                        _ = stopped.changed() => {},
                        _ = tokio::time::sleep(Duration::from_millis(20)) => {},
                    }
                }
            }
        }
    });
    let harness = Harness {
        directory,
        services,
        actor,
        library,
        music,
        folder,
        stop,
        watching,
        worker,
        status,
        jobs_run,
    };
    if mode != WatchMode::Off {
        harness.watched().await;
    }
    harness
}

impl Harness {
    async fn watched(&self) -> WatchBackend {
        let mut status = self.status.clone();
        tokio::time::timeout(WAIT, async {
            loop {
                if let Some(folder) = status.borrow_and_update().get(&self.folder) {
                    return folder.backend;
                }
                status.changed().await.unwrap();
            }
        })
        .await
        .expect("The connected folder must become watched")
    }

    async fn unwatched(&self) {
        let mut status = self.status.clone();
        tokio::time::timeout(WAIT, async {
            while status.borrow_and_update().contains_key(&self.folder) {
                status.changed().await.unwrap();
            }
        })
        .await
        .expect("A disconnected folder must stop being watched");
    }

    async fn recordings(&self) -> usize {
        self.services
            .catalog
            .recordings(&self.actor, Search::default())
            .await
            .unwrap()
            .items
            .iter()
            .filter(|recording| recording.available)
            .count()
    }

    async fn indexed(&self, expected: usize) {
        tokio::time::timeout(WAIT, async {
            while self.recordings().await != expected {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("The folder must index {expected} recordings by itself"));
    }

    async fn settle(&self) {
        tokio::time::sleep(Duration::from_millis(800)).await;
    }

    async fn shutdown(mut self) {
        self.stop.send_replace(true);
        tokio::time::timeout(WAIT, self.watching.take().unwrap())
            .await
            .expect("Shutdown must join every watcher")
            .unwrap();
        assert!(self.status.borrow().is_empty(), "Shutdown must retire every watcher");
        self.worker.await.unwrap();
        drop(self.directory);
    }
}

#[tokio::test]
async fn a_wav_added_to_a_connected_folder_is_indexed_without_a_manual_scan() {
    let harness = start(WatchMode::Auto, 50, false).await;
    #[cfg(target_os = "linux")]
    assert_eq!(harness.watched().await, WatchBackend::Native);
    assert_eq!(harness.recordings().await, 1);
    support::write_wav(&harness.music.join("second.wav"), 1);
    harness.indexed(2).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn removing_a_file_retires_its_recording_by_itself() {
    let harness = start(WatchMode::Auto, 50, false).await;
    std::fs::remove_file(harness.music.join("first.wav")).unwrap();
    harness.indexed(0).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn a_burst_of_new_files_is_one_scan() {
    let harness = start(WatchMode::Auto, 400, false).await;
    for offset in 1..=8 {
        support::write_wav(&harness.music.join(format!("burst-{offset}.wav")), offset);
    }
    harness.indexed(9).await;
    harness.settle().await;
    assert_eq!(harness.jobs_run.load(Ordering::SeqCst), 1);
    harness.shutdown().await;
}

#[tokio::test]
async fn a_change_during_a_running_scan_queues_exactly_one_follow_up() {
    let harness = start(WatchMode::Auto, 50, true).await;
    let hold = harness.directory.path().join("hold");
    std::fs::write(&hold, b"").unwrap();
    support::write_wav(&harness.music.join("second.wav"), 1);
    tokio::time::timeout(WAIT, async {
        while harness
            .services
            .storage
            .folders(&harness.actor, &harness.library)
            .await
            .unwrap()[0]
            .latest_job
            .as_ref()
            .is_none_or(|job| job.state != "running")
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("The first change must start a scan");
    support::write_wav(&harness.music.join("third.wav"), 2);
    support::write_wav(&harness.music.join("fourth.wav"), 3);
    tokio::time::sleep(Duration::from_millis(400)).await;
    std::fs::remove_file(&hold).unwrap();
    harness.indexed(4).await;
    harness.settle().await;
    assert_eq!(harness.jobs_run.load(Ordering::SeqCst), 2);
    harness.shutdown().await;
}

#[tokio::test]
async fn disconnecting_stops_watching_and_reconnecting_resumes_it() {
    let harness = start(WatchMode::Auto, 50, false).await;
    let folder = harness
        .services
        .storage
        .folders(&harness.actor, &harness.library)
        .await
        .unwrap()
        .remove(0);
    harness
        .services
        .storage
        .remove_folder(&harness.actor, &folder.id, folder.revision)
        .await
        .unwrap();
    harness.unwatched().await;
    support::write_wav(&harness.music.join("offline.wav"), 1);
    harness.settle().await;
    assert_eq!(harness.jobs_run.load(Ordering::SeqCst), 0);
    assert_eq!(harness.recordings().await, 0);

    let reconnected = harness
        .services
        .storage
        .connect_folder(
            &harness.actor,
            &harness.library,
            ConnectFolder {
                directory: DirectoryRef {
                    directory_id: "legacy-music".into(),
                    components: Vec::new(),
                },
                name: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(reconnected.id, harness.folder);
    harness.indexed(2).await;
    harness.watched().await;
    let before = harness.jobs_run.load(Ordering::SeqCst);
    support::write_wav(&harness.music.join("online.wav"), 2);
    harness.indexed(3).await;
    assert_eq!(harness.jobs_run.load(Ordering::SeqCst), before + 1);
    harness.shutdown().await;
}

#[tokio::test]
async fn polling_notices_changes_when_native_watching_is_not_used() {
    let harness = start(WatchMode::Poll, 50, false).await;
    assert_eq!(harness.watched().await, WatchBackend::Polling);
    support::write_wav(&harness.music.join("second.wav"), 1);
    harness.indexed(2).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn disabled_watching_never_scans() {
    let harness = start(WatchMode::Off, 50, false).await;
    support::write_wav(&harness.music.join("second.wav"), 1);
    harness.settle().await;
    assert_eq!(harness.recordings().await, 1);
    assert_eq!(harness.jobs_run.load(Ordering::SeqCst), 0);
    harness.shutdown().await;
}

async fn changes_outside_the_granted_root_are_ignored(mode: WatchMode) {
    let harness = start(mode, 50, false).await;
    let outside = harness.directory.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, harness.music.join("linked")).unwrap();
    harness.settle().await;
    let before = harness.jobs_run.load(Ordering::SeqCst);
    support::write_wav(&outside.join("elsewhere.wav"), 1);
    harness.settle().await;
    assert_eq!(harness.jobs_run.load(Ordering::SeqCst), before);
    assert_eq!(harness.recordings().await, 1);
    harness.shutdown().await;
}

#[tokio::test]
async fn native_watching_does_not_follow_links_out_of_the_folder() {
    changes_outside_the_granted_root_are_ignored(WatchMode::Auto).await;
}

#[tokio::test]
async fn polling_does_not_follow_links_out_of_the_folder() {
    changes_outside_the_granted_root_are_ignored(WatchMode::Poll).await;
}

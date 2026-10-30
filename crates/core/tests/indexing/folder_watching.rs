#![cfg(unix)]
use hito_core::test_support::support;
use hito_core::{
    Actor, ConnectFolder, CoreConfig, Credentials, DirectoryRef, FolderWatchSettings, HostDeviceId,
    Search, Services, WatchBackend, WatchCounters, WatchMode, WatchStatus,
};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{sync::watch, task::JoinHandle};

const WAIT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy)]
struct Options {
    mode: WatchMode,
    debounce_ms: u64,
    tool_control: bool,
}

impl Options {
    fn auto() -> Self {
        Self {
            mode: WatchMode::Auto,
            debounce_ms: 50,
            tool_control: false,
        }
    }
}

struct Harness {
    directory: tempfile::TempDir,
    services: Services,
    actor: Actor,
    library: String,
    music: PathBuf,
    folder: String,
    options: Options,
    stop: watch::Sender<bool>,
    watching: Option<JoinHandle<()>>,
    worker: Option<JoinHandle<()>>,
    publish: Option<watch::Sender<WatchStatus>>,
    status: watch::Receiver<WatchStatus>,
    jobs_run: Arc<AtomicUsize>,
}

fn set_modified(path: &Path, time: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn aged(path: &Path) {
    set_modified(path, SystemTime::now() - Duration::from_secs(100));
}

fn later(path: &Path) {
    set_modified(path, SystemTime::now() + Duration::from_secs(10));
}

async fn until(what: &str, mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(WAIT, async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Timed out waiting for {what}"));
}

async fn prepare(options: Options) -> Harness {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    support::write_wav(&music.join("first.wav"), 0);
    aged(&music.join("first.wav"));
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new("watch-test").unwrap(),
    );
    config.folder_watching = FolderWatchSettings {
        mode: options.mode,
        poll_interval: Duration::from_millis(100),
        debounce: Duration::from_millis(options.debounce_ms),
    };
    if options.tool_control {
        let tools = directory.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        let fail = directory.path().join("fail");
        let hold = directory.path().join("hold");
        let tool = tools.join("ffprobe");
        std::fs::write(
            &tool,
            format!(
                "#!/bin/sh\nif [ -e '{}' ]; then exit 1; fi\nif [ \"$1\" != \"-version\" ]; then while [ -e '{}' ]; do sleep 0.05; done; fi\nexec {} \"$@\"\n",
                fail.display(),
                hold.display(),
                hito_core::MediaTools::default().ffprobe.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
        config.media_tools.ffprobe = tool;
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
    support::scan(&services, &actor, &library).await;
    let folder = services
        .storage
        .folders(&actor, &library)
        .await
        .unwrap()
        .remove(0)
        .id;
    let (stop, _) = watch::channel(false);
    let (publish, status) = watch::channel(WatchStatus::new());
    Harness {
        directory,
        services,
        actor,
        library,
        music,
        folder,
        options,
        stop,
        watching: None,
        worker: None,
        publish: Some(publish),
        status,
        jobs_run: Arc::new(AtomicUsize::new(0)),
    }
}

async fn start(options: Options) -> Harness {
    let mut harness = prepare(options).await;
    harness.begin().await;
    harness
}

impl Harness {
    async fn begin(&mut self) {
        let stopped = self.stop.subscribe();
        let publish = self.publish.take().unwrap();
        let storage = self.services.storage.clone();
        self.watching = Some(tokio::spawn({
            let stopped = stopped.clone();
            async move { storage.watch_folders(stopped, publish).await }
        }));
        self.worker = Some(tokio::spawn({
            let storage = self.services.storage.clone();
            let jobs_run = self.jobs_run.clone();
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
        }));
        if self.options.mode != WatchMode::Off {
            let backend = self.watched().await;
            #[cfg(target_os = "linux")]
            if self.options.mode == WatchMode::Auto {
                assert_eq!(backend, WatchBackend::Native);
            }
            let _ = backend;
        }
    }

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

    fn counters(&self) -> Arc<WatchCounters> {
        self.status.borrow()[&self.folder].counters()
    }

    fn jobs(&self) -> usize {
        self.jobs_run.load(Ordering::SeqCst)
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

    async fn latest_state(&self) -> Option<String> {
        self.services
            .storage
            .folders(&self.actor, &self.library)
            .await
            .unwrap()
            .first()
            .and_then(|folder| folder.latest_job.as_ref().map(|job| job.state.clone()))
    }

    async fn latest_job_is(&self, state: &str) {
        tokio::time::timeout(WAIT, async {
            while self.latest_state().await.as_deref() != Some(state) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("The latest scan must become {state}"));
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    async fn shutdown(mut self) {
        self.stop.send_replace(true);
        if let Some(watching) = self.watching.take() {
            tokio::time::timeout(WAIT, watching)
                .await
                .expect("Shutdown must join every watcher")
                .unwrap();
        }
        assert!(
            self.status.borrow().is_empty(),
            "Shutdown must retire every watcher"
        );
        if let Some(worker) = self.worker.take() {
            worker.await.unwrap();
        }
    }
}

#[tokio::test]
async fn a_wav_added_to_a_connected_folder_is_indexed_without_a_manual_scan() {
    let harness = start(Options::auto()).await;
    assert_eq!(harness.recordings().await, 1);
    support::write_wav(&harness.music.join("second.wav"), 1);
    harness.indexed(2).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn removing_a_file_retires_its_recording_by_itself() {
    let harness = start(Options::auto()).await;
    std::fs::remove_file(harness.music.join("first.wav")).unwrap();
    harness.indexed(0).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn a_burst_of_new_files_is_one_scan() {
    let harness = start(Options {
        debounce_ms: 1500,
        ..Options::auto()
    })
    .await;
    for offset in 1..=8 {
        support::write_wav(&harness.music.join(format!("burst-{offset}.wav")), offset);
    }
    harness.indexed(9).await;
    until("the scan to be recorded", || harness.jobs() == 1).await;
    assert_eq!(harness.counters().scans_requested(), 1);
    harness.shutdown().await;
}

#[tokio::test]
async fn a_change_during_a_running_scan_queues_exactly_one_follow_up() {
    let harness = start(Options {
        tool_control: true,
        ..Options::auto()
    })
    .await;
    std::fs::write(harness.path("hold"), b"").unwrap();
    support::write_wav(&harness.music.join("second.wav"), 1);
    harness.latest_job_is("running").await;
    let counters = harness.counters();
    let before = counters.relevant();
    support::write_wav(&harness.music.join("third.wav"), 2);
    support::write_wav(&harness.music.join("fourth.wav"), 3);
    until("the later changes to be seen", || {
        counters.relevant() >= before + 4
    })
    .await;
    std::fs::remove_file(harness.path("hold")).unwrap();
    harness.indexed(4).await;
    until("the follow-up to run", || harness.jobs() == 2).await;
    assert_eq!(counters.scans_requested(), 2);
    harness.shutdown().await;
}

#[tokio::test]
async fn waiting_for_a_running_scan_does_not_hammer_the_database() {
    let harness = start(Options {
        tool_control: true,
        ..Options::auto()
    })
    .await;
    std::fs::write(harness.path("hold"), b"").unwrap();
    support::write_wav(&harness.music.join("second.wav"), 1);
    harness.latest_job_is("running").await;
    let counters = harness.counters();
    support::write_wav(&harness.music.join("third.wav"), 2);
    until("several plain waits", || counters.job_reads() >= 5).await;
    assert_eq!(
        counters.write_requests(),
        1,
        "Waiting must use plain reads, not write attempts"
    );
    std::fs::remove_file(harness.path("hold")).unwrap();
    harness.indexed(3).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn request_errors_back_off_and_recover() {
    use sqlx::Connection;
    let harness = start(Options::auto()).await;
    let database = harness.path("hito.db");
    let set_admin = |admin: bool| {
        let database = database.clone();
        async move {
            let mut connection =
                sqlx::SqliteConnection::connect(&format!("sqlite:{}", database.display()))
                    .await
                    .unwrap();
            sqlx::query("UPDATE users SET admin = ?")
                .bind(admin)
                .execute(&mut connection)
                .await
                .unwrap();
        }
    };
    set_admin(false).await;
    support::write_wav(&harness.music.join("second.wav"), 1);
    let counters = harness.counters();
    until("the first failed request", || {
        counters.request_errors() >= 1
    })
    .await;
    let first = std::time::Instant::now();
    until("the second failed request", || {
        counters.request_errors() >= 2
    })
    .await;
    assert!(
        first.elapsed() >= Duration::from_millis(800),
        "Failures must back off, retried after {:?}",
        first.elapsed()
    );
    set_admin(true).await;
    harness.indexed(2).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn a_failed_watched_scan_is_retried() {
    let harness = start(Options {
        tool_control: true,
        ..Options::auto()
    })
    .await;
    std::fs::write(harness.path("fail"), b"").unwrap();
    support::write_wav(&harness.music.join("second.wav"), 1);
    harness.latest_job_is("failed").await;
    std::fs::remove_file(harness.path("fail")).unwrap();
    harness.indexed(2).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn disconnecting_stops_watching_and_reconnecting_resumes_it() {
    let harness = start(Options::auto()).await;
    let counters = harness.counters();
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
    let requested = counters.scans_requested();
    support::write_wav(&harness.music.join("offline.wav"), 1);
    assert_eq!(harness.jobs(), 0);
    assert_eq!(counters.scans_requested(), requested);
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
    support::write_wav(&harness.music.join("online.wav"), 2);
    harness.indexed(3).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn polling_notices_changes_when_native_watching_is_not_used() {
    let harness = start(Options {
        mode: WatchMode::Poll,
        ..Options::auto()
    })
    .await;
    assert_eq!(harness.watched().await, WatchBackend::Polling);
    support::write_wav(&harness.music.join("second.wav"), 1);
    harness.indexed(2).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn disabled_watching_never_scans() {
    let harness = start(Options {
        mode: WatchMode::Off,
        ..Options::auto()
    })
    .await;
    support::write_wav(&harness.music.join("second.wav"), 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(harness.recordings().await, 1);
    assert_eq!(harness.jobs(), 0);
    harness.shutdown().await;
}

#[tokio::test]
async fn partial_downloads_and_cover_art_do_not_scan_until_they_become_audio() {
    let harness = start(Options::auto()).await;
    let counters = harness.counters();
    support::write_wav(&harness.music.join("download.part"), 1);
    std::fs::write(harness.music.join("cover.jpg"), b"jpeg").unwrap();
    std::fs::write(harness.music.join(".DS_Store"), b"x").unwrap();
    until("the ignored events", || counters.ignored() >= 6).await;
    assert_eq!(counters.relevant(), 0);
    assert_eq!(counters.scans_requested(), 0);
    assert_eq!(harness.jobs(), 0);
    std::fs::rename(
        harness.music.join("download.part"),
        harness.music.join("download.wav"),
    )
    .unwrap();
    harness.indexed(2).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn a_file_added_while_stopped_is_scanned_once_when_watching_starts() {
    let mut harness = prepare(Options::auto()).await;
    support::write_wav(&harness.music.join("second.wav"), 1);
    later(&harness.music.join("second.wav"));
    harness.begin().await;
    harness.indexed(2).await;
    until("the catch-up scan", || harness.jobs() == 1).await;
    assert_eq!(harness.counters().startup_scans(), 1);
    harness.shutdown().await;
}

#[tokio::test]
async fn a_file_removed_while_stopped_is_scanned_when_watching_starts() {
    let mut harness = prepare(Options::auto()).await;
    std::fs::remove_file(harness.music.join("first.wav")).unwrap();
    harness.begin().await;
    harness.indexed(0).await;
    assert_eq!(harness.counters().startup_scans(), 1);
    harness.shutdown().await;
}

#[tokio::test]
async fn nothing_changed_while_stopped_queues_no_scan() {
    let mut harness = prepare(Options::auto()).await;
    harness.begin().await;
    let counters = harness.counters();
    assert_eq!(counters.startup_scans(), 0);
    assert_eq!(counters.scans_requested(), 0);
    assert_eq!(harness.jobs(), 0);
    harness.shutdown().await;
}

#[tokio::test]
async fn native_watching_does_not_see_files_behind_links_out_of_the_folder() {
    let harness = start(Options::auto()).await;
    let outside = harness.path("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, harness.music.join("linked")).unwrap();
    support::write_wav(&outside.join("elsewhere.wav"), 1);
    support::write_wav(&harness.music.join("second.wav"), 2);
    harness.indexed(2).await;
    assert_eq!(harness.counters().outside(), 0);
    harness.shutdown().await;
}

#[tokio::test]
async fn polling_does_not_follow_links_out_of_the_folder() {
    let harness = start(Options {
        mode: WatchMode::Poll,
        ..Options::auto()
    })
    .await;
    let outside = harness.path("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, harness.music.join("linked")).unwrap();
    let counters = harness.counters();
    let polls = counters.polls();
    support::write_wav(&outside.join("elsewhere.wav"), 1);
    until("three more polls", || counters.polls() >= polls + 3).await;
    assert_eq!(counters.scans_requested(), 0);
    assert_eq!(harness.jobs(), 0);
    harness.shutdown().await;
}

#[tokio::test]
async fn a_watcher_that_ends_while_its_folder_is_still_watchable_is_started_again() {
    use sqlx::Connection;
    let harness = start(Options::auto()).await;
    let database = harness.path("hito.db");
    let bind = |bound: bool| {
        let database = database.clone();
        async move {
            let mut connection =
                sqlx::SqliteConnection::connect(&format!("sqlite:{}", database.display()))
                    .await
                    .unwrap();
            sqlx::query("UPDATE music_folders SET bound = ?")
                .bind(bound)
                .execute(&mut connection)
                .await
                .unwrap();
        }
    };
    bind(false).await;
    support::write_wav(&harness.music.join("second.wav"), 1);
    harness.unwatched().await;
    bind(true).await;
    harness.watched().await;
    support::write_wav(&harness.music.join("third.wav"), 2);
    harness.indexed(3).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn moving_a_dotted_directory_out_of_the_folder_retires_its_recordings() {
    let harness = start(Options::auto()).await;
    let album = harness.music.join("Dr.Dre");
    std::fs::create_dir(&album).unwrap();
    support::write_wav(&album.join("track.wav"), 1);
    harness.indexed(2).await;
    let outside = harness.path("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::rename(&album, outside.join("Dr.Dre")).unwrap();
    harness.indexed(1).await;
    harness.shutdown().await;
}

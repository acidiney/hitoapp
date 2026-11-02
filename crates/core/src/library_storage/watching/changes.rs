use super::{
    FolderWatchSettings, WatchBackend, WatchCounters, WatchMode,
    snapshot::{self, Fingerprint},
    volume,
};
use crate::library_storage::{root::Root, walking::supported};
use futures_util::FutureExt;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Notify, Semaphore};

const SLOW_WALK_FACTOR: u32 = 10;

pub(super) struct Changes {
    root: Root,
    folder_id: String,
    interval: Duration,
    wait: Duration,
    slots: Arc<Semaphore>,
    counters: Arc<WatchCounters>,
    changed: Arc<Notify>,
    degraded: Arc<AtomicBool>,
    native: Option<RecommendedWatcher>,
    baseline: Option<u64>,
    cap_logged: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Relevant,
    Ignored,
    Outside,
}

impl Changes {
    pub(super) async fn open(
        root: Root,
        settings: &FolderWatchSettings,
        folder_id: &str,
        slots: Arc<Semaphore>,
        counters: Arc<WatchCounters>,
    ) -> Self {
        let mut changes = Self {
            root,
            folder_id: folder_id.to_owned(),
            interval: settings.poll_interval,
            wait: settings.poll_interval,
            slots,
            counters,
            changed: Arc::new(Notify::new()),
            degraded: Arc::new(AtomicBool::new(false)),
            native: None,
            baseline: None,
            cap_logged: false,
        };
        let reason = match settings.mode {
            WatchMode::Poll => Some("operator_polling"),
            _ => {
                let path = changes.root.path().to_owned();
                tokio::task::spawn_blocking(move || volume::unreliable_for_native_events(&path))
                    .await
                    .unwrap_or(None)
            }
        };
        let reason = match reason {
            Some(reason) => reason,
            None => match changes.start_native().await {
                Ok(watcher) => {
                    changes.native = Some(watcher);
                    return changes;
                }
                Err(reason) => reason,
            },
        };
        tracing::info!(
            event = "folder_watch_polling",
            folder_id,
            reason,
            poll_ms = settings.poll_interval.as_millis() as u64,
            "Music folder changes will be found by periodic polling"
        );
        changes.baseline = changes.fingerprint().await;
        changes
    }

    pub(super) fn backend(&self) -> WatchBackend {
        if self.native.is_some() {
            WatchBackend::Native
        } else {
            WatchBackend::Polling
        }
    }

    async fn start_native(&self) -> Result<RecommendedWatcher, &'static str> {
        let changed = self.changed.clone();
        let degraded = self.degraded.clone();
        let counters = self.counters.clone();
        let root = self.root.path().to_owned();
        tokio::task::spawn_blocking(move || {
            let inside = root.clone();
            let mut watcher = RecommendedWatcher::new(
                move |event: notify::Result<notify::Event>| match event {
                    Ok(event) => match classify(&inside, &event) {
                        Verdict::Relevant => {
                            WatchCounters::add(&counters.relevant);
                            changed.notify_one();
                        }
                        Verdict::Ignored => WatchCounters::add(&counters.ignored),
                        Verdict::Outside => WatchCounters::add(&counters.outside),
                    },
                    Err(_) => {
                        degraded.store(true, Ordering::SeqCst);
                        changed.notify_one();
                    }
                },
                Config::default().with_follow_symlinks(false),
            )
            .map_err(failure)?;
            watcher
                .watch(&root, RecursiveMode::Recursive)
                .map_err(failure)?;
            Ok(watcher)
        })
        .await
        .unwrap_or(Err("native_unavailable"))
    }

    pub(super) fn clear(&self) {
        let _ = self.changed.notified().now_or_never();
    }

    pub(super) async fn next(&mut self) {
        loop {
            if self.native.is_some() {
                self.changed.notified().await;
                if !self.degraded.swap(false, Ordering::SeqCst) {
                    return;
                }
                self.native = None;
                tracing::info!(
                    event = "folder_watch_polling",
                    folder_id = %self.folder_id,
                    reason = "native_watch_failed",
                    "Music folder changes will be found by periodic polling"
                );
                self.baseline = self.fingerprint().await;
                return;
            }
            tokio::time::sleep(self.wait).await;
            let Some(current) = self.fingerprint().await else {
                continue;
            };
            match self.baseline.replace(current) {
                None => {}
                Some(previous) if previous == current => {}
                Some(_) => {
                    WatchCounters::add(&self.counters.relevant);
                    return;
                }
            }
        }
    }

    async fn fingerprint(&mut self) -> Option<u64> {
        let permit = self.slots.clone().acquire_owned().await.ok()?;
        let root = self.root.clone();
        let started = Instant::now();
        let Fingerprint { value, capped } = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            snapshot::fingerprint(&root)
        })
        .await
        .ok()?;
        WatchCounters::add(&self.counters.polls);
        self.wait = self.interval.max(started.elapsed() * SLOW_WALK_FACTOR);
        if capped && !self.cap_logged {
            self.cap_logged = true;
            tracing::warn!(
                event = "folder_watch_file_cap",
                folder_id = %self.folder_id,
                limit = snapshot::MAX_FILES,
                "Music folder has more files than polling compares"
            );
        }
        Some(value)
    }
}

fn failure(error: notify::Error) -> &'static str {
    match error.kind {
        notify::ErrorKind::MaxFilesWatch => "watch_limit_reached",
        _ => "native_unavailable",
    }
}

fn classify(root: &Path, event: &notify::Event) -> Verdict {
    if matches!(event.kind, EventKind::Access(_)) {
        return Verdict::Ignored;
    }
    if event.need_rescan() || matches!(event.kind, EventKind::Other) {
        return Verdict::Relevant;
    }
    let mut inside = false;
    for path in &event.paths {
        if !path.starts_with(root) {
            continue;
        }
        inside = true;
        let is_dir = std::fs::symlink_metadata(path)
            .ok()
            .map(|metadata| metadata.is_dir());
        if matters(path, is_dir) {
            return Verdict::Relevant;
        }
    }
    if inside || event.paths.is_empty() {
        Verdict::Ignored
    } else {
        Verdict::Outside
    }
}

fn matters(path: &Path, is_dir: Option<bool>) -> bool {
    match is_dir {
        Some(true) => true,
        Some(false) => supported(path),
        None => {
            if supported(path) {
                return true;
            }
            !known_noise(path)
        }
    }
}

const NOISE_EXTENSIONS: [&str; 20] = [
    "jpg",
    "jpeg",
    "png",
    "gif",
    "webp",
    "bmp",
    "txt",
    "nfo",
    "cue",
    "log",
    "m3u",
    "m3u8",
    "pdf",
    "part",
    "tmp",
    "crdownload",
    "db",
    "ini",
    "lrc",
    "swp",
];
const NOISE_NAMES: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];

fn known_noise(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    NOISE_NAMES.contains(&name)
        || name.starts_with("._")
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                NOISE_EXTENSIONS
                    .iter()
                    .any(|noise| noise.eq_ignore_ascii_case(extension))
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_storage::walking::paths;
    use std::sync::Mutex;

    #[test]
    fn only_audio_and_directories_matter() {
        let yes = |name: &str, dir| matters(Path::new(name), dir);
        assert!(yes("a/song.MP3", Some(false)));
        assert!(yes("a/Vol. 2", Some(true)));
        assert!(!yes("a/cover.jpg", Some(false)));
        assert!(!yes("a/song.part", Some(false)));
        assert!(!yes(".DS_Store", Some(false)));
        assert!(yes("a/song.flac", None));
        assert!(yes("a/Album", None));
        assert!(yes("a/Vol. 2", None));
        assert!(yes("a/Dr.Dre", None));
        assert!(yes("a/.Hidden Album", None));
        assert!(!yes("a/._song.mp3x", None));
        assert!(!yes("a/cover.jpg", None));
        assert!(!yes("a/.DS_Store", None));
    }

    #[tokio::test]
    async fn a_failed_first_poll_takes_its_baseline_without_signalling() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.wav"), b"x").unwrap();
        let slots = Arc::new(Semaphore::new(1));
        let walks = Arc::new(Semaphore::new(1));
        let root = Root::open(
            directory.path(),
            &slots,
            &walks,
            &Arc::new(Semaphore::new(1)),
            &[],
        )
        .await
        .unwrap();
        let closed = Arc::new(Semaphore::new(1));
        closed.close();
        let settings = FolderWatchSettings {
            mode: WatchMode::Poll,
            poll_interval: Duration::from_millis(10),
            debounce: Duration::from_millis(10),
        };
        let mut changes =
            Changes::open(root, &settings, "folder", closed, Default::default()).await;
        assert!(changes.baseline.is_none());
        changes.slots = Arc::new(Semaphore::new(1));
        assert!(
            tokio::time::timeout(Duration::from_millis(300), changes.next())
                .await
                .is_err(),
            "Taking the first baseline is not a change"
        );
        std::fs::write(directory.path().join("b.wav"), b"x").unwrap();
        tokio::time::timeout(Duration::from_secs(5), changes.next())
            .await
            .expect("A later change must still be found");
    }

    #[tokio::test]
    async fn polling_never_takes_the_walk_slot_a_scan_needs() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.wav"), b"x").unwrap();
        let slots = Arc::new(Semaphore::new(1));
        let walks = Arc::new(Semaphore::new(1));
        let root = Root::open(
            directory.path(),
            &slots,
            &walks,
            &Arc::new(Semaphore::new(1)),
            &[],
        )
        .await
        .unwrap();
        let (entered, wait_entered) = std::sync::mpsc::channel::<()>();
        let (release, wait_release) = std::sync::mpsc::channel::<()>();
        let wait_release = Mutex::new(wait_release);
        let entered = Mutex::new(entered);
        let once = AtomicBool::new(false);
        *snapshot::WALK_HOOK.lock().unwrap() = Some(Arc::new(move || {
            if !once.swap(true, Ordering::SeqCst) {
                entered.lock().unwrap().send(()).unwrap();
                wait_release.lock().unwrap().recv().unwrap();
            }
        }));
        let settings = FolderWatchSettings {
            mode: WatchMode::Poll,
            poll_interval: Duration::from_millis(10),
            debounce: Duration::from_millis(10),
        };
        let opening = tokio::spawn({
            let root = root.clone();
            async move {
                Changes::open(
                    root,
                    &settings,
                    "folder",
                    Arc::new(Semaphore::new(1)),
                    Default::default(),
                )
                .await
            }
        });
        tokio::task::spawn_blocking(move || wait_entered.recv().unwrap())
            .await
            .unwrap();
        let scan_walk = paths(&root);
        let available = scan_walk.is_ok();
        drop(scan_walk);
        release.send(()).unwrap();
        opening.await.unwrap();
        *snapshot::WALK_HOOK.lock().unwrap() = None;
        assert!(
            available,
            "A poll walk must not take the scanner's walk slot"
        );
    }
}

use super::{FolderWatchSettings, WatchBackend, WatchMode, volume};
use crate::library_storage::{root::Root, walking::supported};
use futures_util::FutureExt;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};
use tokio::sync::Notify;

const MAX_DEPTH: usize = 64;
const MAX_ENTRIES: u64 = 2_000_000;

pub(super) struct Changes {
    root: Root,
    interval: Duration,
    changed: Arc<Notify>,
    degraded: Arc<AtomicBool>,
    native: Option<RecommendedWatcher>,
    baseline: Option<u64>,
}

impl Changes {
    pub(super) async fn open(root: Root, settings: &FolderWatchSettings, folder_id: &str) -> Self {
        let changed = Arc::new(Notify::new());
        let degraded = Arc::new(AtomicBool::new(false));
        let mut changes = Self {
            root,
            interval: settings.poll_interval,
            changed,
            degraded,
            native: None,
            baseline: None,
        };
        let reason = match settings.mode {
            WatchMode::Poll => Some("operator_polling"),
            _ => volume::unreliable_for_native_events(changes.root.path()),
        };
        let reason = match reason {
            Some(reason) => reason,
            None => match changes.start_native() {
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
        changes.baseline = Some(changes.fingerprint().await);
        changes
    }

    pub(super) fn backend(&self) -> WatchBackend {
        if self.native.is_some() {
            WatchBackend::Native
        } else {
            WatchBackend::Polling
        }
    }

    fn start_native(&self) -> Result<RecommendedWatcher, &'static str> {
        let changed = self.changed.clone();
        let degraded = self.degraded.clone();
        let mut watcher = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| match event {
                Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
                Ok(_) => changed.notify_one(),
                Err(_) => {
                    degraded.store(true, Ordering::SeqCst);
                    changed.notify_one();
                }
            },
            Config::default().with_follow_symlinks(false),
        )
        .map_err(failure)?;
        watcher
            .watch(self.root.path(), RecursiveMode::Recursive)
            .map_err(failure)?;
        Ok(watcher)
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
                    reason = "native_watch_failed",
                    "Music folder changes will be found by periodic polling"
                );
                self.baseline = Some(self.fingerprint().await);
                return;
            }
            tokio::time::sleep(self.interval).await;
            let current = self.fingerprint().await;
            if self.baseline.replace(current) != Some(current) {
                return;
            }
        }
    }

    async fn fingerprint(&self) -> u64 {
        let Ok(permit) = self.root.walk_slots.clone().try_acquire_owned() else {
            return self.baseline.unwrap_or_default();
        };
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            fingerprint(&root)
        })
        .await
        .unwrap_or_default()
    }
}

fn failure(error: notify::Error) -> &'static str {
    match error.kind {
        notify::ErrorKind::MaxFilesWatch => "watch_limit_reached",
        _ => "native_unavailable",
    }
}

fn fingerprint(root: &Root) -> u64 {
    let mut total = 0u64;
    let mut seen = 0u64;
    let Ok(entries) = root.directory.entries() else {
        return total;
    };
    let mut stack = vec![(PathBuf::new(), entries)];
    while let Some((parent, entries)) = stack.last_mut() {
        let Some(entry) = entries.next() else {
            stack.pop();
            continue;
        };
        seen += 1;
        let Ok(entry) = entry else { continue };
        let relative = parent.join(entry.file_name());
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if stack.len() < MAX_DEPTH
                && seen < MAX_ENTRIES
                && let Ok(children) = root.directory.read_dir(&relative)
            {
                stack.push((relative, children));
            }
        } else if kind.is_file()
            && supported(&relative)
            && let Ok(metadata) = entry.metadata()
        {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.into_std().duration_since(UNIX_EPOCH).ok());
            let mut hasher = DefaultHasher::new();
            (relative, metadata.len(), modified).hash(&mut hasher);
            total = total.wrapping_add(hasher.finish());
        }
    }
    total
}

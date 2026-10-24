mod catch_up;
mod changes;
mod folder;
mod request;
mod snapshot;
mod volume;

use super::{LibraryStorage, folder_types::Folder};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Semaphore, watch},
    task::JoinHandle,
    time::Instant,
};
use tracing::{Instrument, instrument::WithSubscriber};

const DISCOVERY_INTERVAL: Duration = Duration::from_secs(2);
const MAX_RESPAWN_DELAY: Duration = Duration::from_secs(60);
const WALK_PERMITS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchMode {
    Off,
    Auto,
    Poll,
}

#[derive(Clone, Debug)]
pub struct FolderWatchSettings {
    pub mode: WatchMode,
    pub poll_interval: Duration,
    pub debounce: Duration,
}

impl Default for FolderWatchSettings {
    fn default() -> Self {
        Self {
            mode: WatchMode::Auto,
            poll_interval: Duration::from_secs(30),
            debounce: Duration::from_secs(2),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchBackend {
    Native,
    Polling,
}

/// Monotonic per-watcher event counts, readable without locks so tests and
/// diagnostics can wait on real progress instead of sleeping.
#[derive(Debug, Default)]
pub struct WatchCounters {
    pub(super) relevant: AtomicU64,
    pub(super) ignored: AtomicU64,
    pub(super) outside: AtomicU64,
    pub(super) polls: AtomicU64,
    pub(super) scans_requested: AtomicU64,
    pub(super) startup_scans: AtomicU64,
    pub(super) write_requests: AtomicU64,
    pub(super) request_errors: AtomicU64,
}

macro_rules! counter {
    ($($name:ident),*) => {
        impl WatchCounters {
            $(pub fn $name(&self) -> u64 { self.$name.load(Ordering::SeqCst) })*
        }
    };
}
counter!(relevant, ignored, outside, polls, scans_requested, startup_scans, write_requests, request_errors);

impl WatchCounters {
    pub(super) fn add(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Debug)]
pub struct WatchedFolder {
    pub folder_id: String,
    pub generation: i64,
    pub backend: WatchBackend,
    pub counters: Arc<WatchCounters>,
}

pub type WatchStatus = BTreeMap<String, WatchedFolder>;

struct Running {
    generation: i64,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
    reaped: bool,
}

impl Running {
    async fn finish(mut self, folder_id: &str) {
        self.stop.send_replace(true);
        if !self.reaped {
            report(folder_id, (&mut self.task).await);
        }
    }
}

fn report(folder_id: &str, result: Result<(), tokio::task::JoinError>) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(
                event = "folder_watch_task_failed",
                folder_id,
                panicked = error.is_panic(),
                "A music folder watcher stopped unexpectedly"
            );
            false
        }
    }
}

impl LibraryStorage {
    pub async fn watch_folders(
        &self,
        mut stopped: watch::Receiver<bool>,
        status: watch::Sender<WatchStatus>,
    ) {
        if self.watching.mode == WatchMode::Off {
            tracing::info!(
                event = "folder_watching_disabled",
                "Automatic music folder rescans are disabled"
            );
            let _ = stopped.wait_for(|stopped| *stopped).await;
            return;
        }
        tracing::info!(
            event = "folder_watching_started",
            mode = ?self.watching.mode,
            poll_ms = self.watching.poll_interval.as_millis() as u64,
            debounce_ms = self.watching.debounce.as_millis() as u64,
            "Watching connected music folders"
        );
        let status = Arc::new(status);
        let slots = Arc::new(Semaphore::new(WALK_PERMITS));
        let mut running: BTreeMap<String, Running> = BTreeMap::new();
        let mut blocked: BTreeMap<String, (Instant, u32)> = BTreeMap::new();
        let mut interval = tokio::time::interval(DISCOVERY_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            match self.watchable_folders().await {
                Ok(folders) => {
                    self.reconcile(&mut running, &mut blocked, folders, &status, &slots)
                        .await
                }
                Err(error) => tracing::warn!(
                    event = "folder_watch_discovery_failed",
                    error_code = error.code(),
                    "Connected music folders could not be listed for watching"
                ),
            }
            tokio::select! {
                biased;
                _ = stopped.wait_for(|stopped| *stopped) => break,
                _ = interval.tick() => {}
            }
        }
        for (id, watcher) in std::mem::take(&mut running) {
            watcher.finish(&id).await;
            status.send_modify(|folders| {
                folders.remove(&id);
            });
        }
        tracing::info!(
            event = "folder_watching_stopped",
            "Stopped watching music folders"
        );
    }

    async fn watchable_folders(&self) -> crate::Result<Vec<Folder>> {
        Ok(sqlx::query_as(
            "SELECT * FROM music_folders WHERE host_device = ? AND connected = 1 AND bound = 1
               AND directory_id IS NOT NULL ORDER BY id",
        )
        .bind(self.host.as_str())
        .fetch_all(&self.store.pool)
        .await?)
    }

    async fn reconcile(
        &self,
        running: &mut BTreeMap<String, Running>,
        blocked: &mut BTreeMap<String, (Instant, u32)>,
        folders: Vec<Folder>,
        status: &Arc<watch::Sender<WatchStatus>>,
        slots: &Arc<Semaphore>,
    ) {
        for (id, watcher) in running.iter_mut() {
            if watcher.reaped || !watcher.task.is_finished() {
                continue;
            }
            watcher.reaped = true;
            let result = (&mut watcher.task).await;
            if !report(id, result) {
                let failures = blocked.get(id).map_or(1, |(_, failures)| failures + 1);
                let wait = Duration::from_secs(1u64 << failures.min(6)).min(MAX_RESPAWN_DELAY);
                blocked.insert(id.clone(), (Instant::now() + wait, failures));
                watcher.generation = -1;
            }
        }
        let stale: Vec<String> = running
            .iter()
            .filter(|(id, watcher)| {
                !folders
                    .iter()
                    .any(|folder| &folder.id == *id && folder.generation == watcher.generation)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            if let Some(watcher) = running.remove(&id) {
                watcher.finish(&id).await;
            }
            status.send_modify(|folders| {
                folders.remove(&id);
            });
        }
        for folder in folders {
            if running.contains_key(&folder.id)
                || blocked
                    .get(&folder.id)
                    .is_some_and(|(until, _)| *until > Instant::now())
            {
                continue;
            }
            let (stop, stopped) = watch::channel(false);
            let storage = self.clone();
            let id = folder.id.clone();
            let generation = folder.generation;
            let status = status.clone();
            let slots = slots.clone();
            let task = tokio::spawn(
                async move { storage.watch_folder(folder, status, stopped, slots).await }
                    .instrument(tracing::Span::current())
                    .with_current_subscriber(),
            );
            running.insert(
                id,
                Running {
                    generation,
                    stop,
                    task,
                    reaped: false,
                },
            );
        }
    }
}

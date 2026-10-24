mod changes;
mod folder;
mod request;
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
use tokio::{sync::watch, task::JoinHandle};
use tracing::{Instrument, instrument::WithSubscriber};

const DISCOVERY_INTERVAL: Duration = Duration::from_millis(500);

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

#[derive(Debug, Default)]
pub struct WatchCounters {
    relevant: AtomicU64,
    ignored: AtomicU64,
    outside: AtomicU64,
    polls: AtomicU64,
    scans_requested: AtomicU64,
    startup_scans: AtomicU64,
    write_requests: AtomicU64,
    request_errors: AtomicU64,
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
    pub(super) fn bump(counter: &AtomicU64) {
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
}

impl Running {
    async fn finish(self) {
        self.stop.send_replace(true);
        let _ = self.task.await;
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
        let mut running: BTreeMap<String, Running> = BTreeMap::new();
        let mut interval = tokio::time::interval(DISCOVERY_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            match self.watchable_folders().await {
                Ok(folders) => self.reconcile(&mut running, folders, &status).await,
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
            watcher.finish().await;
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
            "SELECT * FROM music_folders WHERE host_device = ? AND connected = 1 AND bound = 1 ORDER BY id",
        )
        .bind(self.host.as_str())
        .fetch_all(&self.store.pool)
        .await?)
    }

    async fn reconcile(
        &self,
        running: &mut BTreeMap<String, Running>,
        folders: Vec<Folder>,
        status: &Arc<watch::Sender<WatchStatus>>,
    ) {
        let stale: Vec<String> = running
            .iter()
            .filter(|(id, watcher)| {
                watcher.task.is_finished()
                    || !folders
                        .iter()
                        .any(|folder| &folder.id == *id && folder.generation == watcher.generation)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            if let Some(watcher) = running.remove(&id) {
                watcher.finish().await;
            }
            status.send_modify(|folders| {
                folders.remove(&id);
            });
        }
        for folder in folders {
            if running.contains_key(&folder.id) {
                continue;
            }
            let (stop, stopped) = watch::channel(false);
            let storage = self.clone();
            let id = folder.id.clone();
            let generation = folder.generation;
            let status = status.clone();
            let task = tokio::spawn(
                async move { storage.watch_folder(folder, status, stopped).await }
                    .instrument(tracing::Span::current())
                    .with_current_subscriber(),
            );
            running.insert(
                id,
                Running {
                    generation,
                    stop,
                    task,
                },
            );
        }
    }
}

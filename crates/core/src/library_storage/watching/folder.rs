use super::{WatchStatus, WatchedFolder, changes::Changes, request::ScanRequest};
use crate::library_storage::{LibraryStorage, folder_types::Folder};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::watch,
    time::{Instant, sleep},
};

const BUSY_RETRY: Duration = Duration::from_millis(200);
const MAX_QUIET_WAITS: u32 = 10;

impl LibraryStorage {
    pub(super) async fn watch_folder(
        &self,
        folder: Folder,
        status: Arc<watch::Sender<WatchStatus>>,
        mut stop: watch::Receiver<bool>,
    ) {
        let settings = &self.watching;
        let Some(selection) = folder.directory() else {
            return;
        };
        let root = loop {
            match self.directories.open_selected(&selection).await {
                Ok(root) => break root,
                Err(error) => {
                    tracing::debug!(
                        event = "folder_watch_unavailable",
                        folder_id = %folder.id,
                        error_code = error.code(),
                        "Music folder is not available to watch yet"
                    );
                    tokio::select! {
                        _ = stop.changed() => return,
                        _ = sleep(settings.poll_interval) => {}
                    }
                }
            }
        };
        let mut changes = Changes::open(root, settings, &folder.id).await;
        let backend = changes.backend();
        status.send_modify(|folders| {
            folders.insert(
                folder.id.clone(),
                WatchedFolder {
                    folder_id: folder.id.clone(),
                    generation: folder.generation,
                    backend,
                    counters: Default::default(),
                },
            );
        });
        tracing::info!(
            event = "folder_watch_started",
            folder_id = %folder.id,
            generation = folder.generation,
            backend = ?backend,
            "Watching music folder for changes"
        );
        loop {
            tokio::select! {
                biased;
                _ = stop.changed() => break,
                () = changes.next() => {}
            }
            if !self.quiet_period(&mut changes, &mut stop).await
                || !self.request_when_idle(&folder, &changes, &mut stop).await
            {
                break;
            }
        }
        tracing::info!(
            event = "folder_watch_stopped",
            folder_id = %folder.id,
            generation = folder.generation,
            "Stopped watching music folder"
        );
    }

    async fn quiet_period(&self, changes: &mut Changes, stop: &mut watch::Receiver<bool>) -> bool {
        let debounce = self.watching.debounce;
        let limit = Instant::now() + debounce * MAX_QUIET_WAITS;
        loop {
            tokio::select! {
                biased;
                _ = stop.changed() => return false,
                () = changes.next() => {
                    tracing::trace!(event = "folder_watch_burst", "More music folder changes arrived");
                    if Instant::now() >= limit {
                        return true;
                    }
                }
                _ = sleep(debounce) => return true,
            }
        }
    }

    async fn request_when_idle(
        &self,
        folder: &Folder,
        changes: &Changes,
        stop: &mut watch::Receiver<bool>,
    ) -> bool {
        loop {
            let started = Instant::now();
            match self
                .request_watched_scan(&folder.id, folder.generation)
                .await
            {
                Ok(ScanRequest::Queued | ScanRequest::Covered) => {
                    changes.clear();
                    tracing::debug!(
                        event = "folder_watch_scan_requested",
                        folder_id = %folder.id,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        "Music folder change requested a scan"
                    );
                    return true;
                }
                Ok(ScanRequest::Superseded) => return false,
                Ok(ScanRequest::Busy) => {}
                Err(error) => tracing::warn!(
                    event = "folder_watch_request_failed",
                    folder_id = %folder.id,
                    error_code = error.code(),
                    "Music folder change could not request a scan"
                ),
            }
            tokio::select! {
                _ = stop.changed() => return false,
                _ = sleep(BUSY_RETRY) => {}
            }
        }
    }
}

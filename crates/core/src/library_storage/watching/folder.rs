use super::{
    WatchCounters, WatchStatus, WatchedFolder,
    changes::Changes,
    request::{JobEnd, ScanRequest},
};
use crate::library_storage::{LibraryStorage, folder_types::Folder};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Semaphore, watch},
    time::{Instant, sleep},
};

const MAX_QUIET_WAITS: u32 = 10;
const MAX_FAILED_SCAN_RETRIES: u32 = 5;
const OPEN_RETRY_START: Duration = Duration::from_millis(250);
const OPEN_RETRY_MAX: Duration = Duration::from_secs(5);
const JOB_POLL_START: Duration = Duration::from_millis(100);
const JOB_POLL_MAX: Duration = Duration::from_secs(2);
const ERROR_BACKOFF_MAX: Duration = Duration::from_secs(30);

fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(1u64 << attempt.saturating_sub(1).min(5)).min(ERROR_BACKOFF_MAX)
}

impl LibraryStorage {
    pub(super) async fn watch_folder(
        &self,
        folder: Folder,
        status: Arc<watch::Sender<WatchStatus>>,
        mut stop: watch::Receiver<bool>,
        slots: Arc<Semaphore>,
    ) {
        let settings = &self.watching;
        let Some(selection) = folder.directory() else {
            return;
        };
        let mut delay = OPEN_RETRY_START;
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
                        _ = sleep(delay) => {}
                    }
                    delay = (delay * 2).min(OPEN_RETRY_MAX);
                }
            }
        };
        let counters = Arc::new(WatchCounters::default());
        let mut changes = Changes::open(
            root.clone(),
            settings,
            &folder.id,
            slots.clone(),
            counters.clone(),
        )
        .await;
        let mut dirty = match self.changed_since_last_scan(&folder, &root, &slots).await {
            Ok(changed) => {
                if changed {
                    WatchCounters::add(&counters.startup_scans);
                }
                changed
            }
            Err(error) => {
                tracing::warn!(
                    event = "folder_watch_catch_up_failed",
                    folder_id = %folder.id,
                    error_code = error.code(),
                    "Music folder could not be compared with its last scan"
                );
                false
            }
        };
        let backend = changes.backend();
        status.send_modify(|folders| {
            folders.insert(
                folder.id.clone(),
                WatchedFolder {
                    folder_id: folder.id.clone(),
                    generation: folder.generation,
                    backend,
                    counters: counters.clone(),
                },
            );
        });
        tracing::info!(
            event = "folder_watch_started",
            folder_id = %folder.id,
            generation = folder.generation,
            backend = ?backend,
            catch_up = dirty,
            "Watching music folder for changes"
        );
        loop {
            if !dirty {
                tokio::select! {
                    biased;
                    _ = stop.changed() => break,
                    () = changes.next() => {}
                }
                if !self.quiet_period(&mut changes, &mut stop).await {
                    break;
                }
            }
            dirty = false;
            if !self
                .request_when_idle(&folder, &changes, &counters, &mut stop)
                .await
            {
                break;
            }
        }
        status.send_modify(|folders| {
            folders.remove(&folder.id);
        });
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
        counters: &WatchCounters,
        stop: &mut watch::Receiver<bool>,
    ) -> bool {
        let mut errors = 0u32;
        let mut failures = 0u32;
        loop {
            let started = Instant::now();
            let request = match self
                .request_watched_scan(&folder.id, folder.generation, counters)
                .await
            {
                Ok(request) => request,
                Err(error) => {
                    errors += 1;
                    WatchCounters::add(&counters.request_errors);
                    if errors == 1 {
                        tracing::warn!(
                            event = "folder_watch_request_failed",
                            folder_id = %folder.id,
                            error_code = error.code(),
                            "Music folder change could not request a scan; retrying with backoff"
                        );
                    }
                    if !pause(stop, backoff(errors)).await {
                        return false;
                    }
                    continue;
                }
            };
            if errors > 0 {
                errors = 0;
                tracing::info!(
                    event = "folder_watch_request_recovered",
                    folder_id = %folder.id,
                    "Music folder scan requests work again"
                );
            }
            let (job, follow_up) = match request {
                ScanRequest::Superseded => return false,
                ScanRequest::Queued(job) => {
                    WatchCounters::add(&counters.scans_requested);
                    changes.clear();
                    tracing::debug!(
                        event = "folder_watch_scan_requested",
                        folder_id = %folder.id,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        "Music folder change requested a scan"
                    );
                    (job, false)
                }
                ScanRequest::Covered(job) => {
                    changes.clear();
                    (job, false)
                }
                ScanRequest::Busy(job) => (job, true),
            };
            match self.await_job(&job, counters, stop).await {
                None => return false,
                Some(_) if follow_up => {}
                Some(JobEnd::Complete) => return true,
                Some(JobEnd::Failed) => {
                    failures += 1;
                    if failures > MAX_FAILED_SCAN_RETRIES {
                        tracing::warn!(
                            event = "folder_watch_scan_gave_up",
                            folder_id = %folder.id,
                            "Watched scans keep failing; waiting for the next change"
                        );
                        return true;
                    }
                    tracing::info!(
                        event = "folder_watch_scan_retry",
                        folder_id = %folder.id,
                        attempt = failures,
                        "A watched scan failed and will be queued again"
                    );
                    if !pause(stop, backoff(failures)).await {
                        return false;
                    }
                }
            }
        }
    }

    async fn await_job(
        &self,
        job: &str,
        counters: &WatchCounters,
        stop: &mut watch::Receiver<bool>,
    ) -> Option<JobEnd> {
        let mut delay = JOB_POLL_START;
        loop {
            WatchCounters::add(&counters.job_reads);
            match self.job_end(job).await {
                Ok(Some(end)) => return Some(end),
                Ok(None) => {}
                Err(error) => tracing::debug!(
                    event = "folder_watch_job_read_failed",
                    error_code = error.code(),
                    "Scan state could not be read"
                ),
            }
            if !pause(stop, delay).await {
                return None;
            }
            delay = (delay * 3 / 2).min(JOB_POLL_MAX);
        }
    }
}

async fn pause(stop: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    tokio::select! {
        _ = stop.changed() => false,
        _ = sleep(duration) => true,
    }
}

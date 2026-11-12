use crate::{Error, process, search};
use hito_plugin_sdk::{FailureTracker, HealthReason, SearchPage, SearchRequest};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, oneshot};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{Instrument, instrument::WithSubscriber};

pub(crate) struct Extractor {
    executable: PathBuf,
    slots: Arc<Semaphore>,
    admission: Mutex<()>,
    workers: TaskTracker,
    stopped: CancellationToken,
    failed: Arc<AtomicBool>,
    health: Arc<FailureTracker>,
}

const HEALTH_FAILURES: usize = 3;
const HEALTH_WINDOW: Duration = Duration::from_secs(120);

impl Extractor {
    pub(crate) fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            slots: Arc::new(Semaphore::new(2)),
            admission: Mutex::new(()),
            workers: TaskTracker::new(),
            stopped: CancellationToken::new(),
            failed: Arc::new(AtomicBool::new(false)),
            health: Arc::new(FailureTracker::new(HEALTH_FAILURES, HEALTH_WINDOW)),
        }
    }

    pub(crate) fn health(&self) -> &FailureTracker {
        &self.health
    }

    pub(crate) async fn search(&self, request: SearchRequest) -> Result<SearchPage, Error> {
        let limit = request.limit;
        let query = request.query.clone();
        self.execute(
            process::Intent::Search {
                query: request.query,
                limit,
            },
            "youtube_search_complete",
            move |bytes| search::normalize(&bytes, &query, limit),
        )
        .await
    }

    pub(crate) async fn recording(&self, id: String) -> Result<crate::recording::Resolved, Error> {
        let owner = id.clone();
        self.execute(
            process::Intent::Recording { id },
            "youtube_recording_complete",
            move |bytes| crate::recording::resolve(&bytes, &owner),
        )
        .await
    }

    pub(crate) fn available(&self) -> bool {
        !self.stopped.is_cancelled() && !self.failed.load(Ordering::Acquire)
    }

    async fn execute<T: Send + 'static>(
        &self,
        intent: process::Intent,
        event: &'static str,
        normalize: impl FnOnce(Vec<u8>) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let receiver = {
            let _gate = self
                .admission
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if self.workers.is_closed() || self.failed.load(Ordering::Acquire) {
                return Err(Error::Unavailable);
            }
            let permit = self
                .slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| Error::Busy)?;
            let executable = self.executable.clone();
            let stopped = self.stopped.clone();
            let failed = self.failed.clone();
            let health = self.health.clone();
            let (mut sender, receiver) = oneshot::channel();
            let dispatch = tracing::dispatcher::get_default(Clone::clone);
            self.workers.spawn(
                async move {
                    let _permit = permit;
                    let started = Instant::now();
                    let canceled = async {
                        tokio::select! {
                            biased;
                            _ = stopped.cancelled() => {},
                            _ = sender.closed() => {},
                        }
                    };
                    let extracted = process::run(executable, intent, canceled).await;
                    match &extracted {
                        Ok(_) => health.record_success(),
                        Err(Error::Network) => health.record_failure(HealthReason::Network),
                        Err(Error::Refused) => health.record_failure(HealthReason::Refused),
                        Err(_) => {}
                    }
                    let result = extracted.and_then(normalize);
                    if matches!(result, Err(Error::CleanupFailed)) {
                        failed.store(true, Ordering::Release);
                        stopped.cancel();
                    }
                    tracing::info!(
                        event,
                        outcome = result
                            .as_ref()
                            .map_or_else(|error| error.outcome(), |_| "complete"),
                        elapsed_ms = started.elapsed().as_millis() as u64
                    );
                    let _ = sender.send(result);
                }
                .instrument(tracing::Span::current())
                .with_subscriber(dispatch),
            );
            receiver
        };
        receiver.await.map_err(|_| Error::Unavailable)?
    }

    pub(crate) async fn shutdown(&self) -> Result<(), Error> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(1);
        let shutdown_id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        {
            let _gate = self
                .admission
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.workers.close();
            self.stopped.cancel();
        }
        self.workers.wait().await;
        let failed = self.failed.load(Ordering::Acquire);
        tracing::info!(
            event = "youtube_shutdown_complete",
            shutdown_id,
            outcome = if failed { "cleanup_failed" } else { "complete" },
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        if failed {
            Err(Error::CleanupFailed)
        } else {
            Ok(())
        }
    }
}

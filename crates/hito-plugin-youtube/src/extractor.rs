use crate::{Error, process, recordings};
use hito_plugin_sdk::{SearchPage, SearchRequest};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
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
}

impl Extractor {
    pub(crate) fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            slots: Arc::new(Semaphore::new(2)),
            admission: Mutex::new(()),
            workers: TaskTracker::new(),
            stopped: CancellationToken::new(),
            failed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) async fn search(&self, request: SearchRequest) -> Result<SearchPage, Error> {
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
                    let result = process::run(executable, request.query, request.limit, canceled)
                        .await
                        .and_then(|bytes| recordings::normalize(&bytes, request.limit));
                    if matches!(result, Err(Error::CleanupFailed)) {
                        failed.store(true, Ordering::Release);
                        stopped.cancel();
                    }
                    tracing::info!(
                        event = "youtube_search_complete",
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

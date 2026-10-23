use hito_core::{Error, Result, SourceResolution};
use std::{
    future::Future,
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};
use tokio::{sync::oneshot, task::JoinSet};
use tracing::{Instrument, instrument::WithSubscriber};

#[derive(Clone, Default)]
pub(crate) struct Resolutions(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    tasks: JoinSet<()>,
    failed: bool,
}

struct Waiter {
    cancel: Option<oneshot::Sender<()>>,
}

impl Drop for Waiter {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
            tracing::debug!(
                event = "http_source_resolution_disconnected",
                outcome = "cancelled",
                "Source resolution response waiter ended"
            );
        }
    }
}

/// Hosted resolutions remain owned through cooperative cleanup and publication.
/// Embedded routers run inline with the embedding caller's lifetime.
pub(crate) async fn run<Operation, Pending>(
    host: Option<Resolutions>,
    operation: Operation,
) -> Result<SourceResolution>
where
    Operation: FnOnce(oneshot::Receiver<()>) -> Pending + Send + 'static,
    Pending: Future<Output = Result<SourceResolution>> + Send + 'static,
{
    let (cancel, cancelled) = oneshot::channel();
    let mut waiter = Waiter {
        cancel: Some(cancel),
    };
    let Some(host) = host else {
        let result = operation(cancelled).await;
        waiter.cancel.take();
        return result;
    };
    let (send, receive) = oneshot::channel();
    let started = Instant::now();
    let work = async move {
        let result = operation(cancelled).await;
        tracing::debug!(
            event = "http_source_resolution_finished",
            outcome = if result.is_ok() {
                "completed"
            } else {
                "rejected"
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Owned HTTP source resolution finished"
        );
        let _ = send.send(result);
    };
    {
        let mut state = host.state();
        state.reap();
        state
            .tasks
            .spawn(work.in_current_span().with_current_subscriber());
    }
    let result = receive
        .await
        .map_err(|_| Error::Internal("Source resolution task did not finish".into()))?;
    waiter.cancel.take();
    result
}

impl Resolutions {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn reap(&self) {
        self.state().reap();
    }

    pub(crate) async fn finish(&self) -> Result<()> {
        let started = Instant::now();
        let (mut tasks, mut failed) = {
            let mut state = self.state();
            (std::mem::take(&mut state.tasks), state.failed)
        };
        tracing::info!(
            event = "http_source_resolutions_draining",
            remaining = tasks.len(),
            "Joining owned HTTP source resolutions"
        );
        while let Some(result) = tasks.join_next().await {
            failed |= result.is_err();
        }
        tracing::info!(
            event = "http_source_resolutions_drained",
            outcome = if failed { "failed" } else { "settled" },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Owned HTTP source resolutions joined"
        );
        if failed {
            Err(Error::Internal(
                "Source resolution task did not finish".into(),
            ))
        } else {
            Ok(())
        }
    }
}

impl State {
    fn reap(&mut self) {
        while let Some(result) = self.tasks.try_join_next() {
            if result.is_err() {
                self.failed = true;
                tracing::error!(
                    event = "http_source_resolution_task_failed",
                    "Owned HTTP source resolution task failed"
                );
            }
        }
    }
}

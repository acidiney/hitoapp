use super::{CHECKPOINT_CAPACITY, Checkpoint, Report};
use crate::{PlayerError, player::Result};
use hito_core::{Action, Collection, Operation, SyncBatch};
use std::{collections::VecDeque, future::Future, pin::Pin, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

type Pending = Pin<Box<dyn Future<Output = (Checkpoint, hito_core::Result<()>)> + Send>>;
pub(super) struct Writer {
    collection: Collection,
    client: String,
    queued: VecDeque<Checkpoint>,
    pending: Option<Pending>,
    retry_at: Option<Instant>,
    closing: bool,
}
impl Writer {
    pub fn new(collection: Collection) -> Self {
        Self {
            collection,
            client: format!("native-{}", Uuid::new_v4()),
            queued: VecDeque::new(),
            pending: None,
            retry_at: None,
            closing: false,
        }
    }
    pub fn load(&self) -> usize {
        self.queued.len() + usize::from(self.pending.is_some())
    }
    pub fn enqueue(&mut self, checkpoint: Checkpoint) -> bool {
        if self.load() >= CHECKPOINT_CAPACITY {
            return false;
        }
        self.queued.push_back(checkpoint);
        true
    }
    pub fn shutdown(&mut self) {
        self.closing = true;
        self.retry_at = None;
    }
    pub async fn next(&mut self) -> Report {
        if self.pending.is_none() {
            if let Some(at) = self.retry_at {
                tokio::time::sleep_until(at).await;
                self.retry_at = None;
            }
            let Some(checkpoint) = self.queued.pop_front() else {
                return std::future::pending().await;
            };
            let collection = self.collection.clone();
            let client = self.client.clone();
            self.pending = Some(Box::pin(async move {
                let started = Instant::now();
                tracing::debug!(event = "player_progress_write_started", operation_id = %checkpoint.id, account_id = checkpoint.actor.user().id, "Saving listening progress");
                let result = collection
                    .apply(
                        &checkpoint.actor,
                        SyncBatch {
                            device_id: client,
                            operations: vec![Operation {
                                id: checkpoint.id,
                                action: Action::ProgressSet {
                                    recording_id: checkpoint.recording,
                                    position_ms: checkpoint.position,
                                    speed: f64::from(checkpoint.speed) / 1000.,
                                },
                            }],
                        },
                    )
                    .await
                    .map(|_| ());
                tracing::debug!(event = "player_progress_write_finished", operation_id = %checkpoint.id, success = result.is_ok(), elapsed_ms = started.elapsed().as_millis() as u64, "Listening progress write finished");
                (checkpoint, result)
            }));
        }
        let (checkpoint, result) = self.pending.as_mut().unwrap().as_mut().await;
        self.pending = None;
        let rejected = matches!(
            result,
            Err(hito_core::Error::Unauthorized
                | hito_core::Error::Forbidden
                | hito_core::Error::NotFound)
        );
        let report = Report {
            scope: checkpoint.key.scope,
            failed: result.is_err(),
            rejected,
        };
        if let Err(error) = result {
            tracing::warn!(event = "player_progress_write_failed", operation_id = %checkpoint.id, error_kind = error.code(), "Listening progress could not be saved");
            if !rejected && !self.closing {
                self.queued.push_front(checkpoint);
                self.retry_at = Some(Instant::now() + Duration::from_millis(250));
            }
        }
        report
    }
    pub fn result(report: &Report) -> Result<()> {
        if report.failed && !report.rejected {
            Err(PlayerError::Persistence)
        } else {
            Ok(())
        }
    }
}

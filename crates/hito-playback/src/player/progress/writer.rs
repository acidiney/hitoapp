use super::{CHECKPOINT_CAPACITY, Checkpoint, Report};
use crate::{PlayerError, player::Result};
use hito_core::{Action, Collection, Operation, SyncBatch};
use std::{collections::VecDeque, future::Future, pin::Pin, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

struct Queued {
    checkpoint: Checkpoint,
    ordinal: u64,
}
type Pending = Pin<Box<dyn Future<Output = (Queued, hito_core::Result<()>)> + Send>>;
pub(super) struct Writer {
    collection: Collection,
    client: String,
    queued: VecDeque<Queued>,
    issued: u64,
    settled: u64,
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
            issued: 0,
            settled: 0,
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
        self.issued = self
            .issued
            .checked_add(1)
            .expect("checkpoint ordinal exhausted");
        self.queued.push_back(Queued {
            checkpoint,
            ordinal: self.issued,
        });
        true
    }
    pub fn watermark(&self) -> u64 {
        self.issued
    }
    pub fn settled(&self, watermark: u64) -> bool {
        self.settled >= watermark
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
            let Some(queued) = self.queued.pop_front() else {
                return std::future::pending().await;
            };
            let collection = self.collection.clone();
            let client = self.client.clone();
            self.pending = Some(Box::pin(async move {
                let checkpoint = &queued.checkpoint;
                let started = Instant::now();
                let mut operations = vec![Operation {
                    id: checkpoint.id,
                    action: Action::ProgressSet {
                        recording_id: checkpoint.recording,
                        position_ms: checkpoint.position,
                        speed: f64::from(checkpoint.speed) / 1000.,
                    },
                }];
                if let Some(chapter) = checkpoint.chapter {
                    operations.push(Operation {
                        id: chapter.id,
                        action: Action::AudiobookChapterSet {
                            chapter: chapter.anchor,
                        },
                    });
                }
                tracing::debug!(event = "player_progress_write_started", operation_id = %checkpoint.id, account_id = checkpoint.actor.user().id, operation_count = operations.len(), "Saving listening progress");
                let result = collection
                    .apply(
                        &checkpoint.actor,
                        SyncBatch {
                            device_id: client,
                            operations,
                        },
                    )
                    .await
                    .map(|_| ());
                tracing::debug!(event = "player_progress_write_finished", operation_id = %checkpoint.id, success = result.is_ok(), elapsed_ms = started.elapsed().as_millis() as u64, "Listening progress write finished");
                (queued, result)
            }));
        }
        let (queued, result) = self.pending.as_mut().unwrap().as_mut().await;
        self.pending = None;
        let rejected = matches!(
            result,
            Err(hito_core::Error::Unauthorized
                | hito_core::Error::Forbidden
                | hito_core::Error::NotFound)
        );
        let checkpoint = &queued.checkpoint;
        if result.is_ok() || rejected || self.closing {
            self.settled = queued.ordinal;
        }
        let report = Report {
            scope: checkpoint.key.scope,
            failed: result.is_err(),
            rejected,
        };
        if let Err(error) = result {
            tracing::warn!(event = "player_progress_write_failed", operation_id = %checkpoint.id, error_kind = error.code(), "Listening progress could not be saved");
            if !rejected && !self.closing {
                self.queued.push_front(queued);
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

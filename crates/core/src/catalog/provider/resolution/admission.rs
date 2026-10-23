use crate::{Error, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{sync::oneshot, time::Instant};
use uuid::Uuid;

#[derive(Clone, Default)]
pub(crate) struct CallerAdmission {
    active: Arc<Mutex<HashMap<String, Slot>>>,
}

enum Phase {
    Running,
    Cancelling,
}

struct Slot {
    generation: Uuid,
    phase: Phase,
    successor: Option<Successor>,
}

struct Successor {
    generation: Uuid,
    ready: oneshot::Sender<()>,
}

pub(super) struct Caller {
    active: Arc<Mutex<HashMap<String, Slot>>>,
    account: String,
    generation: Uuid,
    ready: Option<oneshot::Receiver<()>>,
}

impl CallerAdmission {
    pub(super) fn enter(&self, account: &str) -> Result<Caller> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| Error::Internal("Source resolution admission is unavailable".into()))?;
        let generation = Uuid::new_v4();
        let ready = if let Some(slot) = active.get_mut(account) {
            if !matches!(slot.phase, Phase::Cancelling) || slot.successor.is_some() {
                drop(active);
                return Err(busy());
            }
            let (send, receive) = oneshot::channel();
            slot.successor = Some(Successor {
                generation,
                ready: send,
            });
            Some(receive)
        } else {
            if active.len() >= 2 {
                drop(active);
                return Err(busy());
            }
            active.insert(
                account.to_owned(),
                Slot {
                    generation,
                    phase: Phase::Running,
                    successor: None,
                },
            );
            None
        };
        let count = active.len();
        let pending = active
            .values()
            .filter(|slot| slot.successor.is_some())
            .count();
        drop(active);
        tracing::debug!(
            event = if ready.is_some() {
                "source_resolution_handoff_waiting"
            } else {
                "source_resolution_caller_admitted"
            },
            active_callers = count as u64,
            pending_callers = pending as u64,
            "Source resolution caller reserved"
        );
        Ok(Caller {
            active: Arc::clone(&self.active),
            account: account.to_owned(),
            generation,
            ready,
        })
    }
}

impl Caller {
    pub(super) async fn wait(&mut self) -> Result<()> {
        if let Some(ready) = self.ready.take() {
            let started = Instant::now();
            ready
                .await
                .map_err(|_| Error::Internal("Source resolution handoff did not finish".into()))?;
            tracing::debug!(
                event = "source_resolution_handoff_admitted",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Source resolution cancellation handoff completed"
            );
        }
        Ok(())
    }

    pub(super) fn cancelling(&self) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(slot) = active.get_mut(&self.account)
            && slot.generation == self.generation
        {
            slot.phase = Phase::Cancelling;
        }
    }
}

impl Drop for Caller {
    fn drop(&mut self) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let remove = if let Some(slot) = active.get_mut(&self.account) {
            if slot.generation == self.generation {
                if let Some(successor) = slot.successor.take() {
                    slot.generation = successor.generation;
                    slot.phase = Phase::Running;
                    successor.ready.send(()).is_err()
                } else {
                    true
                }
            } else {
                if slot
                    .successor
                    .as_ref()
                    .is_some_and(|successor| successor.generation == self.generation)
                {
                    slot.successor = None;
                }
                false
            }
        } else {
            false
        };
        if remove {
            active.remove(&self.account);
        }
    }
}

fn busy() -> Error {
    tracing::debug!(
        event = "source_resolution_busy",
        outcome = "busy",
        "Source resolution caller capacity is busy"
    );
    Error::Conflict("Source resolution is busy. Try again.".into())
}

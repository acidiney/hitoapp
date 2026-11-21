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
    preempt: Option<oneshot::Sender<()>>,
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
    background: bool,
}

impl CallerAdmission {
    pub(super) fn enter(&self, account: &str) -> Result<Caller> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| Error::Internal("Source resolution admission is unavailable".into()))?;
        let generation = Uuid::new_v4();
        let ready = if let Some(slot) = active.get_mut(account) {
            if let Some(preempt) = slot.preempt.take() {
                let _ = preempt.send(());
                slot.phase = Phase::Cancelling;
                tracing::debug!(
                    event = "source_resolution_background_preempted",
                    "A listener's resolution asked background preparation to stop"
                );
            }
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
                    preempt: None,
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
            background: false,
        })
    }
}

impl CallerAdmission {
    pub(super) fn enter_background(
        &self,
        account: &str,
    ) -> Result<Option<(Caller, oneshot::Receiver<()>)>> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| Error::Internal("Source resolution admission is unavailable".into()))?;
        if !active.is_empty() {
            return Ok(None);
        }
        let generation = Uuid::new_v4();
        let (preempt, preempted) = oneshot::channel();
        active.insert(
            account.to_owned(),
            Slot {
                generation,
                phase: Phase::Running,
                successor: None,
                preempt: Some(preempt),
            },
        );
        Ok(Some((
            Caller {
                active: Arc::clone(&self.active),
                account: account.to_owned(),
                generation,
                ready: None,
                background: true,
            },
            preempted,
        )))
    }
}

impl Caller {
    pub(super) fn background(&self) -> bool {
        self.background
    }

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
                    slot.preempt = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_listener_takes_over_from_background_preparation() {
        let admission = CallerAdmission::default();
        let (background, mut preempted) = admission.enter_background("listener").unwrap().unwrap();
        assert!(
            admission.enter_background("other").unwrap().is_none(),
            "background preparation only starts when nothing else is resolving"
        );
        let other = admission.enter("other").unwrap();
        let mut listener = admission.enter("listener").unwrap();
        assert!(
            preempted.try_recv().is_ok(),
            "the background caller is asked to stop"
        );
        drop(background);
        listener.wait().await.unwrap();
        drop(other);
    }
}

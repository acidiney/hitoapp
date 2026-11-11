use super::{Entry, stopped};
use crate::{Actor, Error, Result};
use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak, atomic::Ordering},
};
use uuid::Uuid;

pub(super) type Completion = Shared<BoxFuture<'static, std::result::Result<(), Arc<Error>>>>;

pub(super) struct Work {
    pub owner: Actor,
    pub finished: tokio::task::AbortHandle,
    pub correlation: u64,
    pub completion: Completion,
}

#[derive(Default)]
pub(crate) struct Registry {
    pub(super) state: Mutex<State>,
}

#[derive(Default)]
pub(super) struct State {
    #[cfg(test)]
    pub lifetime: Option<std::time::Duration>,
    #[cfg(test)]
    pub admission_pause: Option<Arc<super::testing::AdmissionPause>>,
    #[cfg(test)]
    pub publication_pause: Option<Arc<super::testing::AdmissionPause>>,
    #[cfg(test)]
    pub read_pause: Option<Arc<super::testing::ReadPause>>,
    pub closed: bool,
    pub closing: Option<Completion>,
    pub entries: HashMap<Uuid, Arc<Entry>>,
    pub work: HashMap<Uuid, Work>,
}

impl State {
    pub fn reserve(&mut self, actor: &Actor, entry: Arc<Entry>) -> Result<()> {
        if self.closed {
            return Err(stopped());
        }
        self.work.retain(|_, work| !work.finished.is_finished());
        let owned = self
            .entries
            .values()
            .filter(|entry| entry.actor.id() == actor.id());
        let owned_count = owned.clone().count();
        let pending_owned = owned
            .filter(|entry| !entry.ready.load(Ordering::Acquire))
            .count();
        let pending = self
            .entries
            .values()
            .filter(|entry| !entry.ready.load(Ordering::Acquire))
            .count();
        if self.entries.len() >= 32 || owned_count >= 8 || pending >= 8 || pending_owned >= 5 {
            return Err(Error::Conflict(
                "Playback leases are busy. Try again.".into(),
            ));
        }
        self.entries.insert(entry.metadata.id, entry);
        Ok(())
    }
}

impl Registry {
    pub(super) fn entry(&self, actor: &Actor, id: Uuid) -> Result<Arc<Entry>> {
        let state = self.state.lock().map_err(|_| stopped())?;
        if state.closed {
            return Err(stopped());
        }
        state
            .entries
            .get(&id)
            .filter(|entry| entry.ready.load(Ordering::Acquire) && actor.same_session(&entry.actor))
            .cloned()
            .ok_or(Error::NotFound)
    }

    pub(super) fn closing(&self) -> Result<Completion> {
        let mut state = self.state.lock().map_err(|_| stopped())?;
        if let Some(completion) = &state.closing {
            return Ok(completion.clone());
        }
        state.closed = true;
        for entry in state.entries.values() {
            entry.stop();
        }
        let work: Vec<_> = state
            .work
            .drain()
            .map(|(_, work)| work.completion)
            .collect();
        let task = tokio::spawn(async move {
            let mut first = None;
            for completion in work {
                if let Err(error) = completion.await {
                    first.get_or_insert(error);
                }
            }
            first.map_or(Ok(()), Err)
        });
        let completion = async move { task.await.map_err(|_| Arc::new(stopped()))? }
            .boxed()
            .shared();
        state.closing = Some(completion.clone());
        Ok(completion)
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        if let Ok(state) = self.state.get_mut() {
            for entry in state.entries.values() {
                entry.stop();
            }
        }
    }
}

pub(super) struct Retirement {
    pub entry: Arc<Entry>,
    pub registry: Weak<Registry>,
}
impl Drop for Retirement {
    fn drop(&mut self) {
        self.entry.stop();
        if let Some(registry) = self.registry.upgrade()
            && let Ok(mut state) = registry.state.lock()
        {
            state.entries.remove(&self.entry.metadata.id);
        }
        tracing::debug!(
            event = "playback_lease_closed",
            playback_lease = self.entry.correlation,
            admitted = self.entry.ready.load(Ordering::Acquire),
            elapsed_ms = self.entry.started.elapsed().as_millis() as u64,
            "Playback lease resources retired"
        );
    }
}

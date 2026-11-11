use super::Entry;
use crate::{Error, Result};
use std::sync::{Condvar, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, Semaphore};

pub(super) struct AdmissionPause {
    entered: Semaphore,
    released: Semaphore,
}
impl Default for AdmissionPause {
    fn default() -> Self {
        Self {
            entered: Semaphore::new(0),
            released: Semaphore::new(0),
        }
    }
}
impl AdmissionPause {
    pub async fn pause(&self, entry: &Entry) -> Result<()> {
        self.entered.add_permits(1);
        let mut stop = entry.cancel.subscribe();
        entry.alive()?;
        tokio::select! {
            biased;
            _=stop.changed()=>Err(Error::NotFound),
            result=tokio::time::timeout(Duration::from_secs(5),self.released.acquire())=> {
                result.map_err(|_| Error::Internal("Playback admission test gate timed out".into()))?.map_err(|_| Error::NotFound)?.forget();
                Ok(())
            },
        }
    }
    pub async fn entered(&self, count: u32) {
        self.entered.acquire_many(count).await.unwrap().forget();
    }
    pub fn release(&self, count: usize) {
        self.released.add_permits(count);
    }
}

#[derive(Default)]
pub(super) struct ReadPause {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
    entered: Notify,
}
impl ReadPause {
    pub fn pause(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.entered.notify_waiters();
        let (state, result) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(5), |state| !state.1)
            .unwrap();
        if result.timed_out() && !state.1 {
            Err(Error::Internal("Playback read test gate timed out".into()))
        } else {
            Ok(())
        }
    }
    pub async fn entered(&self) {
        loop {
            let waiting = self.entered.notified();
            if self.state.lock().unwrap().0 {
                return;
            }
            waiting.await;
        }
    }
    pub fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

#[derive(Default)]
pub(super) struct Publications {
    pub created: Notify,
    pub opened: Notify,
    pub read: Notify,
}

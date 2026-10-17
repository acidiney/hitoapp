use std::sync::{Arc, Mutex, Weak};

use tokio::sync::watch;

use super::files::StateOwner;

struct Workers {
    closing: bool,
    active: usize,
}

pub(super) struct WorkerTracker {
    state: Mutex<Workers>,
    active: watch::Sender<usize>,
}

pub(super) struct WorkerLease {
    owner: Option<Arc<StateOwner>>,
    tracker: Arc<WorkerTracker>,
}

impl WorkerTracker {
    pub(super) fn new() -> Self {
        let (active, _) = watch::channel(0);
        Self {
            state: Mutex::new(Workers {
                closing: false,
                active: 0,
            }),
            active,
        }
    }

    pub(super) fn lease(
        self: &Arc<Self>,
        owner: &Weak<StateOwner>,
    ) -> Result<WorkerLease, sqlx::Error> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closing {
            return Err(sqlx::Error::PoolClosed);
        }
        let owner = owner.upgrade().ok_or(sqlx::Error::PoolClosed)?;
        state.active = state.active.checked_add(1).ok_or(sqlx::Error::PoolClosed)?;
        self.active.send_replace(state.active);
        Ok(WorkerLease {
            owner: Some(owner),
            tracker: self.clone(),
        })
    }

    pub(super) fn close_gate(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closing = true;
    }

    pub(super) async fn wait_empty(&self) {
        let mut active = self.active.subscribe();
        let _ = active.wait_for(|active| *active == 0).await;
    }
}

impl Drop for WorkerLease {
    fn drop(&mut self) {
        drop(self.owner.take());
        let mut state = self
            .tracker
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.active -= 1;
        self.tracker.active.send_replace(state.active);
    }
}

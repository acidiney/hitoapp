use crate::{Error, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub(super) struct Accounts {
    slots: Arc<Mutex<HashMap<String, Weak<Semaphore>>>>,
    limit: usize,
}

impl Default for Accounts {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Accounts {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            slots: Default::default(),
            limit,
        }
    }
    pub(super) fn enter(&self, account: &str) -> Result<OwnedSemaphorePermit> {
        let mut accounts = self
            .slots
            .lock()
            .map_err(|_| Error::Internal("Plugin admission is unavailable".into()))?;
        accounts.retain(|_, slot| slot.strong_count() > 0);
        let slot = accounts
            .get(account)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let slot = Arc::new(Semaphore::new(self.limit));
                accounts.insert(account.to_owned(), Arc::downgrade(&slot));
                slot
            });
        slot.try_acquire_owned().map_err(|_| {
            Error::Conflict("Provider requests for this account are busy. Try again.".into())
        })
    }
}

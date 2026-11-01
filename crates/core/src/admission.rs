use crate::{Error, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub(crate) struct Accounts {
    slots: Arc<Mutex<HashMap<String, Weak<Semaphore>>>>,
    limit: usize,
}

impl Default for Accounts {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Accounts {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            slots: Default::default(),
            limit,
        }
    }
    pub(crate) fn enter(&self, account: &str) -> Result<OwnedSemaphorePermit> {
        self.try_enter(account)?.ok_or_else(Self::busy)
    }

    pub(crate) fn try_enter(&self, account: &str) -> Result<Option<OwnedSemaphorePermit>> {
        Ok(self.slot(account)?.try_acquire_owned().ok())
    }

    pub(crate) async fn enter_within(
        &self,
        account: &str,
        wait: Duration,
    ) -> Result<OwnedSemaphorePermit> {
        let slot = self.slot(account)?;
        tokio::time::timeout(wait, slot.acquire_owned())
            .await
            .ok()
            .and_then(|permit| permit.ok())
            .ok_or_else(Self::busy)
    }

    fn busy() -> Error {
        Error::Conflict("Provider requests for this account are busy. Try again.".into())
    }

    fn slot(&self, account: &str) -> Result<Arc<Semaphore>> {
        let mut accounts = self
            .slots
            .lock()
            .map_err(|_| Error::Internal("Plugin admission is unavailable".into()))?;
        accounts.retain(|_, slot| slot.strong_count() > 0);
        Ok(accounts
            .get(account)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let slot = Arc::new(Semaphore::new(self.limit));
                accounts.insert(account.to_owned(), Arc::downgrade(&slot));
                slot
            }))
    }
}

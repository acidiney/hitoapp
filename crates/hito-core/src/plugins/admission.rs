use crate::{Error, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Default)]
pub(super) struct Accounts(Arc<Mutex<HashMap<String, Weak<Semaphore>>>>);

impl Accounts {
    pub(super) fn enter(&self, account: &str) -> Result<OwnedSemaphorePermit> {
        let mut accounts = self
            .0
            .lock()
            .map_err(|_| Error::Internal("Plugin admission is unavailable".into()))?;
        accounts.retain(|_, slot| slot.strong_count() > 0);
        let slot = accounts
            .get(account)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let slot = Arc::new(Semaphore::new(1));
                accounts.insert(account.to_owned(), Arc::downgrade(&slot));
                slot
            });
        slot.try_acquire_owned().map_err(|_| {
            Error::Conflict("A plugin connection is already being checked for your account".into())
        })
    }
}

use crate::{Error, Result};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub(crate) struct CallerAdmission {
    active: Arc<Mutex<HashSet<String>>>,
}

pub(super) struct Caller {
    active: Arc<Mutex<HashSet<String>>>,
    account: String,
}

impl CallerAdmission {
    pub(super) fn enter(&self, account: &str) -> Result<Caller> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| Error::Internal("Source resolution admission is unavailable".into()))?;
        if active.contains(account) || active.len() >= 2 {
            drop(active);
            tracing::debug!(
                event = "source_resolution_busy",
                outcome = "busy",
                "Source resolution caller capacity is busy"
            );
            return Err(Error::Conflict(
                "Source resolution is busy. Try again.".into(),
            ));
        }
        active.insert(account.to_owned());
        let count = active.len();
        drop(active);
        let caller = Caller {
            active: Arc::clone(&self.active),
            account: account.to_owned(),
        };
        tracing::debug!(
            event = "source_resolution_caller_admitted",
            active_callers = count as u64,
            "Source resolution caller admitted"
        );
        Ok(caller)
    }
}

impl Drop for Caller {
    fn drop(&mut self) {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.account);
    }
}

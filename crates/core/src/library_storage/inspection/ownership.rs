use crate::{Error, Result};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub(in crate::library_storage) struct Scope {
    slots: Arc<Semaphore>,
    _owner: Arc<dyn Send + Sync>,
}

pub(super) struct ScopePermit {
    _permit: OwnedSemaphorePermit,
    _scope: Scope,
}

impl Scope {
    pub(in crate::library_storage) fn new<T: Send + Sync + 'static>(
        slots: Arc<Semaphore>,
        owner: Arc<T>,
    ) -> Self {
        Self {
            slots,
            _owner: owner,
        }
    }

    pub(super) async fn acquire(&self) -> Result<ScopePermit> {
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Internal("Media inspection stopped".into()))?;
        Ok(ScopePermit {
            _permit: permit,
            _scope: self.clone(),
        })
    }
}

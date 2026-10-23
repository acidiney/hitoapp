use super::ControlClient;
use crate::{Error, Result};
use hito_plugin_sdk::{RecordingRequest, SourcePage};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(crate) struct SourceBatch {
    account: String,
    owner: Arc<Semaphore>,
    requests: Semaphore,
    parallelism: usize,
    _account: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

impl SourceBatch {
    pub(crate) fn parallelism(&self) -> usize {
        self.parallelism
    }
}

impl ControlClient {
    pub(in crate::plugins) fn source_batch(
        &self,
        account: &str,
        bindings: usize,
    ) -> Result<SourceBatch> {
        let parallelism = bindings.clamp(1, 2);
        let permit = self.accounts.enter(account)?;
        let global = self
            .slots
            .clone()
            .try_acquire_many_owned(parallelism as u32)
            .map_err(|_| Error::Conflict("Plugin connections are busy. Try again.".into()))?;
        Ok(SourceBatch {
            account: account.to_owned(),
            owner: self.slots.clone(),
            requests: Semaphore::new(parallelism),
            parallelism,
            _account: permit,
            _global: global,
        })
    }

    pub(in crate::plugins) async fn sources_in_batch(
        &self,
        batch: &SourceBatch,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        request: &RecordingRequest,
    ) -> Result<SourcePage> {
        if batch.account != account || !Arc::ptr_eq(&batch.owner, &self.slots) {
            return Err(Error::Internal(
                "Provider source admission does not match its request".into(),
            ));
        }
        let _request = batch
            .requests
            .try_acquire()
            .map_err(|_| Error::Conflict("Provider source batch is busy. Try again.".into()))?;
        self.source_page(base_url, token, request).await
    }
}

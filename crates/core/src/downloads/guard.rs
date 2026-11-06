use super::{Downloads, lease::Lease};
use crate::{Error, Result};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::sync::watch;

pub(super) struct Guard<'a> {
    lease: &'a Lease,
    downloads: &'a Downloads,
    stopped: &'a mut watch::Receiver<bool>,
    pub(super) bytes: u64,
    pub(super) total: Option<u64>,
    checked: Instant,
    renewed: Instant,
}

impl<'a> Guard<'a> {
    pub(super) fn new(
        lease: &'a Lease,
        downloads: &'a Downloads,
        stopped: &'a mut watch::Receiver<bool>,
    ) -> Self {
        Self {
            lease,
            downloads,
            stopped,
            bytes: 0,
            total: None,
            checked: Instant::now(),
            renewed: Instant::now(),
        }
    }

    async fn check(&mut self) -> Result<()> {
        if self.checked.elapsed() >= Duration::from_millis(250) {
            self.lease.check(&self.downloads.store).await?;
            self.checked = Instant::now();
        }
        if self.renewed.elapsed() >= Duration::from_secs(1) {
            tokio::time::timeout(
                Duration::from_secs(1),
                self.lease
                    .heartbeat(&self.downloads.store, self.bytes, self.total),
            )
            .await
            .unwrap_or(Err(Error::Upstream(
                "Download progress could not be saved in time",
            )))?;
            self.renewed = Instant::now();
        }
        Ok(())
    }

    pub(super) async fn wait<T>(
        &mut self,
        pending: impl Future<Output = Result<T>>,
    ) -> Result<Option<T>> {
        if *self.stopped.borrow() {
            return Ok(None);
        }
        self.check().await?;
        tokio::pin!(pending);
        loop {
            tokio::select! {
                biased;
                _ = self.stopped.changed() => { if *self.stopped.borrow() { return Ok(None); } },
                result = &mut pending => { self.check().await?; return result.map(Some); },
                _ = tokio::time::sleep(Duration::from_millis(250)) => self.check().await?,
            }
        }
    }

    pub(super) async fn publish<T>(
        &mut self,
        pending: impl Future<Output = Result<T>>,
    ) -> Result<Option<T>> {
        let mut withdrawn = false;
        let mut authority_error = None;
        tokio::pin!(pending);
        loop {
            tokio::select! {
                result = &mut pending => {
                    let value = result?;
                    if let Some(error) = authority_error { return Err(error); }
                    return Ok((!withdrawn).then_some(value));
                },
                _ = self.stopped.changed(), if !withdrawn => { withdrawn = *self.stopped.borrow(); },
                _ = tokio::time::sleep(Duration::from_millis(250)), if authority_error.is_none() => {
                    if let Err(error) = self.check().await { authority_error = Some(error); }
                },
            }
        }
    }
}

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
    pub(super) yielded: Option<watch::Receiver<bool>>,
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
            yielded: None,
            checked: Instant::now(),
            renewed: Instant::now(),
        }
    }

    async fn check(&mut self) -> Result<()> {
        if self.checked.elapsed() >= self.downloads.authority_interval {
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
        if self
            .yielded
            .as_ref()
            .is_some_and(|yielded| *yielded.borrow())
        {
            return Err(super::yielded());
        }
        self.check().await?;
        tokio::pin!(pending);
        loop {
            tokio::select! {
                biased;
                _ = self.stopped.changed() => { if *self.stopped.borrow() { return Ok(None); } },
                () = playback_arrived(&mut self.yielded) => return Err(super::yielded()),
                result = &mut pending => { self.check().await?; return result.map(Some); },
                _ = tokio::time::sleep(self.downloads.authority_interval) => self.check().await?,
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
                _ = tokio::time::sleep(self.downloads.authority_interval), if authority_error.is_none() => {
                    if let Err(error) = self.check().await { authority_error = Some(error); }
                },
            }
        }
    }
}

async fn playback_arrived(yielded: &mut Option<watch::Receiver<bool>>) {
    let Some(yielded) = yielded else {
        return std::future::pending().await;
    };
    loop {
        if *yielded.borrow_and_update() {
            return;
        }
        if yielded.changed().await.is_err() && !*yielded.borrow() {
            return std::future::pending().await;
        }
    }
}

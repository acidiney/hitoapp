use super::{Downloads, lease::Lease};
use crate::{
    Error, OpenedAudio, Result,
    library_storage::{MAX_MANAGED_BYTES, ManagedFileLocation},
};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::sync::watch;

impl Downloads {
    /// Wait for managed filesystem work after callers and transfer workers have stopped.
    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn finish_downloads(&self) -> Result<()> {
        self.files.settle().await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn run_next_job_until(
        &self,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<bool> {
        let (stop, stopped) = watch::channel(false);
        let (done, finished) = tokio::sync::oneshot::channel::<()>();
        let job = async move {
            let result = self.run_job(stopped).await;
            drop(done);
            result
        };
        let cancellation = async move {
            tokio::select! { _ = shutdown => { stop.send_replace(true); }, _ = finished => {} }
        };
        let (result, ()) = tokio::join!(job, cancellation);
        result
    }

    async fn run_job(&self, mut stopped: watch::Receiver<bool>) -> Result<bool> {
        if *stopped.borrow() {
            return Ok(false);
        }
        let reclaimed = match self.reclaim_pending().await {
            Ok(reclaimed) => reclaimed,
            Err(error) => {
                tracing::debug!(
                    error_code = error.code(),
                    "Pending download cleanup deferred"
                );
                false
            }
        };
        let Some(lease) = self.claim().await? else {
            return Ok(reclaimed);
        };
        let started = Instant::now();
        let result = self.receive(&lease, &mut stopped).await;
        lease.retire(&self.store).await?;
        let outcome = match result {
            Ok(Some(location)) => match self.publish_ready(&lease, &location).await {
                Ok(()) => "ready",
                Err(error) => {
                    let stopping = *stopped.borrow();
                    self.finish_failed(&lease, Some(error), stopping).await?
                }
            },
            Ok(None) => self.finish_failed(&lease, None, true).await?,
            Err(error) => {
                let stopping = *stopped.borrow();
                self.finish_failed(&lease, Some(error), stopping).await?
            }
        };
        if outcome != "ready"
            && let Err(error) = self.reclaim_pending().await
        {
            tracing::debug!(error_code = error.code(), "Download cleanup deferred");
        }
        tracing::info!(job_id = %lease.id, intent_id = %lease.intent_id, account_id = %lease.account_id,
            attempt = lease.attempts, outcome, elapsed_ms = started.elapsed().as_millis() as u64, "Download transfer finished");
        Ok(true)
    }

    async fn receive(
        &self,
        lease: &Lease,
        stopped: &mut watch::Receiver<bool>,
    ) -> Result<Option<ManagedFileLocation>> {
        let mut guard = Guard::new(lease, self, stopped);
        lease.check(&self.store).await?;
        let Some(opened) = guard
            .wait(self.plugins.open_download(&lease.account_id, &lease.origin))
            .await?
        else {
            return Ok(None);
        };
        let OpenedAudio::Content(mut content) = opened else {
            return Err(Error::Upstream(
                "The download service did not return complete audio",
            ));
        };
        if content.range.is_some()
            || content
                .length
                .is_some_and(|length| length == 0 || length > MAX_MANAGED_BYTES)
        {
            return Err(Error::Upstream("The download audio exceeds its size limit"));
        }
        guard.total = content.length;
        let mut body = content
            .body
            .take()
            .ok_or(Error::Upstream("The download service did not return audio"))?;
        let Some(mut pending) = guard.wait(self.files.begin(lease.reservation()?)).await? else {
            return Ok(None);
        };
        loop {
            let Some(chunk) = guard.wait(body.chunk()).await? else {
                return Ok(None);
            };
            let Some(chunk) = chunk else {
                break;
            };
            if chunk.len() as u64 > MAX_MANAGED_BYTES.saturating_sub(guard.bytes) {
                return Err(Error::Upstream("The download audio exceeds its size limit"));
            }
            if guard.wait(pending.write_chunk(&chunk)).await?.is_none() {
                return Ok(None);
            }
            guard.bytes += chunk.len() as u64;
        }
        if guard.bytes == 0 {
            return Err(Error::Upstream("The download service returned empty audio"));
        }
        lease
            .heartbeat(&self.store, guard.bytes, guard.total)
            .await?;
        self.mark_verifying(lease).await?;
        let Some(verified) = guard.wait(self.files.verify(pending)).await? else {
            return Ok(None);
        };
        self.record_verified(lease, verified.location()).await?;
        lease.check(&self.store).await?;
        let location = guard.publish(self.files.publish(verified)).await?;
        Ok(location)
    }
}

struct Guard<'a> {
    lease: &'a Lease,
    downloads: &'a Downloads,
    stopped: &'a mut watch::Receiver<bool>,
    bytes: u64,
    total: Option<u64>,
    checked: Instant,
    renewed: Instant,
}

impl<'a> Guard<'a> {
    fn new(
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

    async fn wait<T>(&mut self, pending: impl Future<Output = Result<T>>) -> Result<Option<T>> {
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

    async fn publish<T>(&mut self, pending: impl Future<Output = Result<T>>) -> Result<Option<T>> {
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

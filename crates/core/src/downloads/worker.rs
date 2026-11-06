use super::{Downloads, guard::Guard, lease::Lease, transfer::Representation};
use crate::{AudioRequest, Error, OpenedAudio, Result, library_storage::ManagedFileLocation};
use std::{future::Future, time::Instant};
use tokio::sync::watch;

impl Downloads {
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
        let outcome = match self.settle(&lease, result, &stopped).await {
            Ok(outcome) => outcome,
            Err(error) => {
                if error.is_storage_full() {
                    self.active.block_on_storage(&lease.lease_token)?;
                }
                return Err(error);
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

    async fn settle(
        &self,
        lease: &Lease,
        result: Result<Option<ManagedFileLocation>>,
        stopped: &watch::Receiver<bool>,
    ) -> Result<&'static str> {
        match result {
            Ok(Some(location)) => match self.publish_ready(lease, &location).await {
                Ok(()) => Ok("ready"),
                Err(error) => {
                    let stopping = *stopped.borrow();
                    self.finish_failed(lease, Some(error), stopping).await
                }
            },
            Ok(None) => self.finish_failed(lease, None, true).await,
            Err(error) => {
                let stopping = *stopped.borrow();
                self.finish_failed(lease, Some(error), stopping).await
            }
        }
    }

    async fn receive(
        &self,
        lease: &Lease,
        stopped: &mut watch::Receiver<bool>,
    ) -> Result<Option<ManagedFileLocation>> {
        let mut guard = Guard::new(lease, self, stopped);
        lease.check(&self.store).await?;
        let Some(opened) = guard
            .wait(self.open_origin(lease, AudioRequest::default()))
            .await?
        else {
            return Ok(None);
        };
        let OpenedAudio::Content(content) = opened else {
            return Err(Error::Upstream(
                "The download service did not return complete audio",
            ));
        };
        let (body, representation) = Representation::admit(content)?;
        guard.total = representation.total();
        let Some(mut pending) = guard.wait(self.files.begin(lease.reservation()?)).await? else {
            return Ok(None);
        };
        if Box::pin(self.copy_audio(lease, &mut guard, &mut pending, body, representation))
            .await?
            .is_none()
        {
            return Ok(None);
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

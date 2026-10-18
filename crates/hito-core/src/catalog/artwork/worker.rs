use super::{
    Catalog,
    assets::{self, ValidatedArtwork},
    jobs,
    lease::Lease,
};
use crate::{Error, Result, plugins::ProviderArtwork};
use hito_plugin_sdk::Capability;
use std::time::{Duration, Instant};
use tokio::sync::watch;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn run_next_artwork_job_until(
        &self,
        shutdown: impl std::future::Future<Output = ()> + Send,
    ) -> Result<bool> {
        let (stop, stopped) = watch::channel(false);
        let (done, finished) = tokio::sync::oneshot::channel::<()>();
        let job = async move {
            let result = self.run_artwork_job(stopped).await;
            drop(done);
            result
        };
        let cancellation = async move {
            tokio::select! {
                _ = shutdown => { stop.send_replace(true); },
                _ = finished => {},
            }
        };
        let (result, ()) = tokio::join!(job, cancellation);
        result
    }

    async fn run_artwork_job(&self, mut stopped: watch::Receiver<bool>) -> Result<bool> {
        if *stopped.borrow() {
            return Ok(false);
        }
        let Ok(_worker_permit) = self.provider_artwork_slots.clone().try_acquire_owned() else {
            return Ok(false);
        };
        let Some(lease) = self.claim_artwork_job().await? else {
            return Ok(false);
        };
        let started = Instant::now();
        let result = self.receive_artwork(&lease, &mut stopped).await;
        let stopping = *stopped.borrow();
        let status = self.finish_artwork_job(&lease, result, stopping).await?;
        tracing::info!(job_id = %lease.id, account_id = %lease.user_id, attempt = lease.attempts,
            outcome = status, elapsed_ms = started.elapsed().as_millis() as u64, "Artwork attempt finished");
        Ok(true)
    }

    async fn receive_artwork(
        &self,
        lease: &Lease,
        stopped: &mut watch::Receiver<bool>,
    ) -> Result<Option<ValidatedArtwork>> {
        let Some(remote) = self.download_artwork(lease, stopped).await? else {
            return Ok(None);
        };
        if *stopped.borrow() {
            return Ok(None);
        }
        lease.actor(&self.store).await?;
        let acquiring = self.artwork_slots.clone().acquire_owned();
        tokio::pin!(acquiring);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let permit = loop {
            tokio::select! {
                biased;
                _ = stopped.changed() => {
                    if *stopped.borrow() { return Ok(None); }
                },
                permit = &mut acquiring => break permit.map_err(|_| Error::Internal("Artwork validation stopped".into()))?,
                _ = heartbeat.tick() => { lease.actor(&self.store).await?; },
            }
        };
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            assets::decode(remote)
        })
        .await
        .map_err(|_| Error::Internal("Artwork validation stopped".into()))?
        .map(Some)
    }

    async fn download_artwork(
        &self,
        lease: &Lease,
        stopped: &mut watch::Receiver<bool>,
    ) -> Result<Option<ProviderArtwork>> {
        if *stopped.borrow() {
            return Ok(None);
        }
        let actor = lease.actor(&self.store).await?;
        let operation = tokio::time::timeout(
            Duration::from_secs(1),
            self.plugins.operation(
                &actor,
                &lease.installation_id,
                lease.installation_revision,
                &[Capability::Artwork],
            ),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Artwork provider authorization could not be checked in time",
        )))?;
        if operation.service != lease.service_id
            || operation.service_revision != lease.service_revision
        {
            return Err(Error::Conflict("This artwork provider changed".into()));
        }
        lease.actor(&self.store).await?;
        let request = lease.target.request();
        let receiving = operation.artwork(&request);
        tokio::pin!(receiving);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = stopped.changed() => {
                    if *stopped.borrow() { return Ok(None); }
                },
                result = &mut receiving => return result,
                _ = heartbeat.tick() => { lease.actor(&self.store).await?; },
            }
        }
    }

    async fn finish_artwork_job(
        &self,
        lease: &Lease,
        result: Result<Option<ValidatedArtwork>>,
        stopped: bool,
    ) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        jobs::maintain(&mut tx).await?;
        let status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM artwork_fetches WHERE id = ? AND lease_token = ?",
        )
        .bind(&lease.id)
        .bind(&lease.lease_token)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(status) = status else {
            tx.commit().await?;
            return Ok("superseded");
        };
        if status != "running" {
            sqlx::query("UPDATE artwork_fetches SET lease_token = NULL,lease_until = NULL WHERE id = ? AND lease_token = ?")
                .bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
            tx.commit().await?;
            return Ok("cancelled");
        }
        let current = match lease.check_on(&mut tx).await {
            Ok(_) => true,
            Err(Error::Conflict(_) | Error::Unauthorized | Error::NotFound) => false,
            Err(error) => return Err(error),
        };
        if !current {
            sqlx::query("UPDATE artwork_fetches SET status = 'cancelled',reservation_bytes = 0,lease_token = NULL,lease_until = NULL,error = 'authority_changed',updated_at = unixepoch() WHERE id = ? AND lease_token = ?")
                .bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
            tx.commit().await?;
            return Ok("cancelled");
        }
        let (status, error, reservation, delay) = if stopped || matches!(&result, Ok(None)) {
            sqlx::query("UPDATE artwork_fetches SET attempts = max(attempts - 1,0) WHERE id = ? AND lease_token = ?")
                .bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
            ("queued", None, jobs::RESERVATION_BYTES, 0i64)
        } else {
            match result {
                Ok(Some(image)) => {
                    assets::publish(&mut tx, &lease.user_id, &lease.target, &lease.id, image)
                        .await?;
                    ("ready", None, 0, 0)
                }
                Err(error) => {
                    tracing::debug!(job_id = %lease.id, error_code = error.code(), "Artwork attempt did not publish");
                    if lease.attempts < 3
                        && matches!(error, Error::Upstream(_) | Error::Conflict(_))
                    {
                        (
                            "queued",
                            Some("provider_unavailable"),
                            jobs::RESERVATION_BYTES,
                            1i64 << lease.attempts,
                        )
                    } else {
                        let code = match error {
                            Error::NotFound => "not_available",
                            Error::Invalid(_) => "invalid_artwork",
                            _ => "retrieval_failed",
                        };
                        ("failed", Some(code), 0, 0)
                    }
                }
                Ok(None) => unreachable!(),
            }
        };
        sqlx::query("UPDATE artwork_fetches SET status = ?,error = ?,reservation_bytes = ?,next_attempt_at = unixepoch() + ?,lease_token = NULL,lease_until = NULL,updated_at = unixepoch() WHERE id = ? AND lease_token = ? AND status = 'running'")
            .bind(status).bind(error).bind(reservation).bind(delay).bind(&lease.id).bind(&lease.lease_token)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(status)
    }
}

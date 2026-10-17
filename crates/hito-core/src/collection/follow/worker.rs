use super::{Collection, history, lease::Lease, persist};
use crate::{
    Error, Result,
    plugins::{PlaylistOperation, PlaylistSnapshot},
};
use hito_plugin_sdk::PlaylistDetailRequest;
use std::time::{Duration, Instant};
use tokio::sync::watch;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn run_next_follow_refresh_until(
        &self,
        shutdown: impl std::future::Future<Output = ()> + Send,
    ) -> Result<bool> {
        let (stop, stopped) = watch::channel(false);
        let (done, finished) = tokio::sync::oneshot::channel::<()>();
        let job = async move {
            let result = self.run_follow_job(stopped).await;
            drop(done);
            result
        };
        let cancellation = async move {
            tokio::select! { _ = shutdown => { stop.send_replace(true); }, _ = finished => {} }
        };
        let (result, ()) = tokio::join!(job, cancellation);
        result
    }

    async fn run_follow_job(&self, mut stopped: watch::Receiver<bool>) -> Result<bool> {
        if *stopped.borrow() {
            return Ok(false);
        }
        let Some(lease) = self.claim_follow_job().await? else {
            return Ok(false);
        };
        let started = Instant::now();
        let result = self.receive_follow_snapshot(&lease, &mut stopped).await;
        let stopping = *stopped.borrow();
        let outcome = self.finish_follow_job(&lease, result, stopping).await?;
        tracing::info!(job_id = %lease.id, account_id = %lease.user_id, attempt = lease.attempts,
            outcome, elapsed_ms = started.elapsed().as_millis() as u64, "Playlist refresh finished");
        Ok(true)
    }

    async fn receive_follow_snapshot(
        &self,
        lease: &Lease,
        stopped: &mut watch::Receiver<bool>,
    ) -> Result<Option<(PlaylistOperation, PlaylistSnapshot)>> {
        if *stopped.borrow() {
            return Ok(None);
        }
        lease.check(&self.store).await?;
        let operation = self
            .plugins
            .playlist_operation(
                &lease.user_id,
                &lease.installation_id,
                lease.installation_revision,
            )
            .await?;
        if operation.service != lease.service_id
            || operation.service_revision != lease.service_revision
            || operation.provider != lease.provider
        {
            return Err(Error::Conflict("This playlist connection changed".into()));
        }
        let snapshot = {
            let receiving = operation.refresh_snapshot(
                PlaylistDetailRequest::Canonical {
                    playlist_id: lease.provider_playlist_id.clone(),
                },
                || async {
                    lease.check(&self.store).await?;
                    operation
                        .recheck(&mut *self.store.pool.acquire().await?)
                        .await
                },
            );
            tokio::pin!(receiving);
            let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    biased;
                    _ = stopped.changed() => { if *stopped.borrow() { return Ok(None); } },
                    result = &mut receiving => break result?,
                    _ = heartbeat.tick() => { lease.check(&self.store).await?; },
                }
            }
        };
        if snapshot.detail.id != lease.provider_playlist_id {
            return Err(Error::Upstream(
                "The provider returned a different playlist",
            ));
        }
        Ok(Some((operation, snapshot)))
    }

    async fn finish_follow_job(
        &self,
        lease: &Lease,
        result: Result<Option<(PlaylistOperation, PlaylistSnapshot)>>,
        stopped: bool,
    ) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playlist_follow_jobs WHERE id = ? AND lease_token = ? AND status = 'running')")
            .bind(&lease.id).bind(&lease.lease_token).fetch_one(&mut *tx).await?;
        if !owned {
            tx.commit().await?;
            return Ok("superseded");
        }
        let current = match lease.check_on(&mut tx).await {
            Ok(()) => true,
            Err(Error::Conflict(_) | Error::NotFound | Error::Unauthorized) => false,
            Err(error) => return Err(error),
        };
        let (status, error, delay, refund) = if !current {
            ("cancelled", Some("authority_changed"), 0, false)
        } else if stopped || matches!(&result, Ok(None)) {
            ("queued", None, 0, true)
        } else {
            let publication = match result {
                Ok(Some((operation, snapshot))) => {
                    operation.recheck(&mut tx).await?;
                    sqlx::query("SAVEPOINT hito_follow_publication")
                        .execute(&mut *tx)
                        .await?;
                    let result =
                        persist::refresh(&mut tx, &operation, &lease.playlist_id, snapshot).await;
                    if result.is_err() {
                        sqlx::query("ROLLBACK TO hito_follow_publication")
                            .execute(&mut *tx)
                            .await?;
                    }
                    sqlx::query("RELEASE hito_follow_publication")
                        .execute(&mut *tx)
                        .await?;
                    result
                }
                Err(error) => Err(error),
                Ok(None) => unreachable!(),
            };
            match publication {
                Ok(()) => ("complete", None, 0, false),
                Err(error) => {
                    tracing::debug!(job_id = %lease.id, error_code = error.code(), "Follow snapshot did not publish");
                    if lease.attempts < 3
                        && matches!(error, Error::Upstream(_) | Error::Conflict(_))
                    {
                        (
                            "queued",
                            Some("provider_unavailable"),
                            1i64 << lease.attempts,
                            false,
                        )
                    } else {
                        ("failed", Some("refresh_failed"), 0, false)
                    }
                }
            }
        };
        sqlx::query("UPDATE playlist_follow_jobs SET status = ?,error = ?,next_attempt_at = unixepoch() + ?,attempts = max(attempts - ?,0),lease_token = NULL,lease_until = NULL,updated_at = unixepoch() WHERE id = ? AND lease_token = ?")
            .bind(status).bind(error).bind(delay).bind(i64::from(refund)).bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
        sqlx::query("UPDATE playlist_follows SET error = ?,next_refresh_at = CASE WHEN ? IN ('failed','cancelled') THEN unixepoch() + interval_seconds ELSE next_refresh_at END WHERE playlist_id = ? AND generation = ?")
            .bind(error).bind(status).bind(&lease.playlist_id).bind(lease.generation).execute(&mut *tx).await?;
        history::prune(&mut tx, &lease.playlist_id).await?;
        tx.commit().await?;
        Ok(status)
    }
}

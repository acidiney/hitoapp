mod jobs;
mod publication;
mod receive;

use super::Podcasts;
use crate::{Error, Result};
use jobs::Lease;
use receive::ReceivedFeed;
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::sync::watch;

impl Podcasts {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn run_next_refresh_until(
        &self,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<bool> {
        let (stop, stopped) = watch::channel(false);
        let (done, finished) = tokio::sync::oneshot::channel::<()>();
        let work = async {
            let result = self.run_refresh(stopped).await;
            drop(done);
            result
        };
        let cancellation = async {
            tokio::select! { _ = shutdown => { stop.send_replace(true); }, _ = finished => {} }
        };
        let (result, ()) = tokio::join!(work, cancellation);
        result
    }

    async fn run_refresh(&self, mut stopped: watch::Receiver<bool>) -> Result<bool> {
        if *stopped.borrow() {
            return Ok(false);
        }
        let Some(lease) = self.claim().await? else {
            return Ok(false);
        };
        let started = Instant::now();
        let result = {
            let receiving = tokio::time::timeout(Duration::from_secs(20), self.receive(&lease));
            tokio::pin!(receiving);
            let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    biased;
                    _ = stopped.changed() => { if *stopped.borrow() { break Ok(None); } },
                    result = &mut receiving => break result.unwrap_or(Err(Error::Upstream("The podcast feed did not finish in time"))).map(Some),
                    _ = heartbeat.tick() => { if let Err(error) = lease.check(&self.store).await { break Err(error); } },
                }
            }
        };
        let stopping = *stopped.borrow();
        let outcome = self.finish(&lease, result, stopping).await?;
        tracing::info!(job_id = %lease.id, subscription_id = %lease.subscription_id, account_id = %lease.user_id, attempt = lease.attempts, outcome, elapsed_ms = started.elapsed().as_millis() as u64, "Podcast refresh finished");
        Ok(true)
    }

    async fn finish(
        &self,
        lease: &Lease,
        result: Result<Option<ReceivedFeed>>,
        stopping: bool,
    ) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM podcast_refresh_jobs WHERE id=? AND status='running' AND lease_token=?)")
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
        let (status, reason, delay, refund) = if !current {
            ("cancelled", Some("authority_changed"), 0, false)
        } else if stopping || matches!(&result, Ok(None)) {
            ("queued", None, 0, true)
        } else {
            let publication = match result {
                Ok(Some(feed)) => {
                    sqlx::query("SAVEPOINT hito_podcast_publication")
                        .execute(&mut *tx)
                        .await?;
                    let result = publication::commit(&mut tx, lease, feed).await;
                    if result.is_err() {
                        sqlx::query("ROLLBACK TO hito_podcast_publication")
                            .execute(&mut *tx)
                            .await?;
                    }
                    sqlx::query("RELEASE hito_podcast_publication")
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
                    tracing::debug!(job_id = %lease.id, error_code = error.code(), "Podcast refresh did not publish");
                    if lease.attempts < 3 {
                        (
                            "queued",
                            Some("feed_unavailable"),
                            1i64 << lease.attempts,
                            false,
                        )
                    } else {
                        ("failed", Some("feed_unavailable"), 0, false)
                    }
                }
            }
        };
        sqlx::query("UPDATE podcast_refresh_jobs SET status=?,error=?,attempts=max(attempts-?,0),next_attempt_at=unixepoch()+?,lease_token=NULL,lease_until=NULL,updated_at=unixepoch() WHERE id=? AND lease_token=?")
            .bind(status).bind(reason).bind(i64::from(refund)).bind(delay).bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
        sqlx::query("UPDATE podcast_subscriptions SET error=?,next_refresh_at=CASE WHEN ? IN ('failed','cancelled') THEN unixepoch()+3600 ELSE next_refresh_at END WHERE id=? AND generation=?")
            .bind(reason).bind(status).bind(&lease.subscription_id).bind(lease.generation).execute(&mut *tx).await?;
        jobs::prune(&mut tx, &lease.subscription_id).await?;
        tx.commit().await?;
        Ok(status)
    }
}

fn stored_number(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::Upstream("The podcast feed contains an oversized episode number"))
}

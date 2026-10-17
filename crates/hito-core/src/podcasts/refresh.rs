use super::{
    Podcasts,
    feed::{self, EpisodeIdentity, FeedSnapshot},
};
use crate::{Error, Result, remote_http::ConditionalValidators};
use sqlx::SqliteConnection;
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::sync::watch;
use uuid::Uuid;

const MAX_FEED_BYTES: usize = 8 * 1024 * 1024;

#[derive(sqlx::FromRow)]
struct Lease {
    id: String,
    subscription_id: String,
    user_id: String,
    library_id: String,
    generation: i64,
    lease_token: String,
    attempts: i64,
    fetch_url: String,
}

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
                    _ = heartbeat.tick() => { if let Err(error) = self.check(&lease).await { break Err(error); } },
                }
            }
        };
        let stopping = *stopped.borrow();
        let outcome = self.finish(&lease, result, stopping).await?;
        tracing::info!(job_id = %lease.id, subscription_id = %lease.subscription_id, account_id = %lease.user_id, attempt = lease.attempts, outcome, elapsed_ms = started.elapsed().as_millis() as u64, "Podcast refresh finished");
        Ok(true)
    }

    async fn claim(&self) -> Result<Option<Lease>> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("UPDATE podcast_refresh_jobs SET status=CASE WHEN attempts>=3 THEN 'failed' ELSE 'queued' END,lease_token=NULL,lease_until=NULL,error='lease_expired',updated_at=unixepoch() WHERE id IN (SELECT id FROM podcast_refresh_jobs WHERE status='running' AND lease_until<=unixepoch() ORDER BY lease_until,id LIMIT 32)")
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE podcast_refresh_jobs SET status='cancelled',error='authority_changed',updated_at=unixepoch() WHERE status='queued' AND NOT EXISTS(SELECT 1 FROM podcast_subscriptions p JOIN current_podcast_authority a ON a.id=p.id WHERE p.id=podcast_refresh_jobs.subscription_id AND p.generation=podcast_refresh_jobs.generation AND p.user_id=podcast_refresh_jobs.user_id)")
            .execute(&mut *tx).await?;
        let id: Option<String> = sqlx::query_scalar("SELECT j.id FROM podcast_refresh_jobs j WHERE j.status='queued' AND j.attempts<3 AND j.next_attempt_at<=unixepoch() AND NOT EXISTS(SELECT 1 FROM podcast_refresh_jobs active WHERE active.user_id=j.user_id AND active.status='running') AND (SELECT count(*) FROM podcast_refresh_jobs active WHERE active.status='running')<2 ORDER BY j.created_at,j.id LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some(id) = id else {
            tx.commit().await?;
            return Ok(None);
        };
        sqlx::query("UPDATE podcast_refresh_jobs SET status='running',attempts=attempts+1,lease_token=?,lease_until=unixepoch()+90,error=NULL,updated_at=unixepoch() WHERE id=? AND status='queued'")
            .bind(Uuid::new_v4().to_string()).bind(&id).execute(&mut *tx).await?;
        let lease = sqlx::query_as("SELECT j.id,j.subscription_id,j.user_id,j.generation,j.lease_token,j.attempts,p.library_id,p.fetch_url FROM podcast_refresh_jobs j JOIN podcast_subscriptions p ON p.id=j.subscription_id AND p.user_id=j.user_id WHERE j.id=?")
            .bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(lease))
    }

    async fn receive(&self, lease: &Lease) -> Result<FeedSnapshot> {
        self.check(lease).await?;
        let remote = self
            .remote
            .get(&lease.fetch_url, ConditionalValidators::default())
            .await?;
        let mut response = remote.response;
        if response.status() != reqwest::StatusCode::OK {
            return Err(Error::Upstream("The podcast feed is unavailable"));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_FEED_BYTES as u64)
        {
            return Err(Error::Upstream("The podcast feed exceeds its size limit"));
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
        {
            return Err(Error::Upstream(
                "The podcast feed uses an unsupported encoding",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Upstream("The podcast feed could not be read"))?
        {
            if chunk.len() > MAX_FEED_BYTES.saturating_sub(bytes.len()) {
                return Err(Error::Upstream("The podcast feed exceeds its size limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        self.check(lease).await?;
        let snapshot = feed::parse(&bytes, &remote.final_url)?;
        for episode in &snapshot.episodes {
            stored_number(episode.duration_ms.unwrap_or(0))?;
            episode.length.map(stored_number).transpose()?;
        }
        Ok(snapshot)
    }

    async fn check(&self, lease: &Lease) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(1), async {
            check_on(&mut *self.store.pool.acquire().await?, lease).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Podcast refresh authority could not be checked in time",
        )))
    }

    async fn finish(
        &self,
        lease: &Lease,
        result: Result<Option<FeedSnapshot>>,
        stopping: bool,
    ) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM podcast_refresh_jobs WHERE id=? AND status='running' AND lease_token=?)")
            .bind(&lease.id).bind(&lease.lease_token).fetch_one(&mut *tx).await?;
        if !owned {
            tx.commit().await?;
            return Ok("superseded");
        }
        let current = match check_on(&mut tx, lease).await {
            Ok(()) => true,
            Err(Error::Conflict(_) | Error::NotFound | Error::Unauthorized) => false,
            Err(error) => return Err(error),
        };
        let (status, reason, delay, refund) = if !current {
            ("cancelled", Some("authority_changed"), 0, false)
        } else if stopping || matches!(&result, Ok(None)) {
            ("queued", None, 0, true)
        } else {
            match result {
                Ok(Some(snapshot)) => {
                    publish(&mut tx, lease, snapshot).await?;
                    ("complete", None, 0, false)
                }
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
                Ok(None) => unreachable!(),
            }
        };
        sqlx::query("UPDATE podcast_refresh_jobs SET status=?,error=?,attempts=max(attempts-?,0),next_attempt_at=unixepoch()+?,lease_token=NULL,lease_until=NULL,updated_at=unixepoch() WHERE id=? AND lease_token=?")
            .bind(status).bind(reason).bind(i64::from(refund)).bind(delay).bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
        sqlx::query("UPDATE podcast_subscriptions SET error=? WHERE id=? AND generation=?")
            .bind(reason)
            .bind(&lease.subscription_id)
            .bind(lease.generation)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(status)
    }
}

async fn check_on(connection: &mut SqliteConnection, lease: &Lease) -> Result<()> {
    let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM podcast_refresh_jobs j JOIN podcast_subscriptions p ON p.id=j.subscription_id AND p.user_id=j.user_id AND p.generation=j.generation JOIN current_podcast_authority a ON a.id=p.id WHERE j.id=? AND j.subscription_id=? AND j.user_id=? AND j.generation=? AND j.status='running' AND j.lease_token=? AND j.lease_until>unixepoch())")
        .bind(&lease.id).bind(&lease.subscription_id).bind(&lease.user_id).bind(lease.generation).bind(&lease.lease_token).fetch_one(connection).await?;
    if current {
        Ok(())
    } else {
        Err(Error::Conflict(
            "This podcast refresh is no longer current".into(),
        ))
    }
}

async fn publish(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    lease: &Lease,
    snapshot: FeedSnapshot,
) -> Result<()> {
    for episode in snapshot.episodes {
        let (kind, value) = match episode.identity {
            EpisodeIdentity::Guid(value) => ("guid", value),
            EpisodeIdentity::Enclosure(value) => ("enclosure", value),
        };
        let existing: Option<String> = sqlx::query_scalar("SELECT recording_id FROM podcast_episodes WHERE subscription_id=? AND identity_kind=? AND identity_value=?")
            .bind(&lease.subscription_id).bind(kind).bind(&value).fetch_optional(&mut **transaction).await?;
        let artist = episode
            .author
            .as_deref()
            .or(snapshot.author.as_deref())
            .unwrap_or(&snapshot.title);
        let duration = stored_number(episode.duration_ms.unwrap_or(0))?;
        let length = episode.length.map(stored_number).transpose()?;
        let recording = if let Some(existing) = existing {
            sqlx::query(
                "UPDATE recordings SET title=?,artist=?,duration_ms=? WHERE id=? AND library_id=?",
            )
            .bind(&episode.title)
            .bind(artist)
            .bind(duration)
            .bind(&existing)
            .bind(&lease.library_id)
            .execute(&mut **transaction)
            .await?;
            existing
        } else {
            let recording = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO recordings(id,library_id,title,artist,duration_ms,provenance) VALUES(?,?,?,?,?,'rss_feed')")
                .bind(&recording).bind(&lease.library_id).bind(&episode.title).bind(artist).bind(duration).execute(&mut **transaction).await?;
            recording
        };
        crate::metadata::refresh_recording_credits(transaction, &recording).await?;
        let episode_id: String = sqlx::query_scalar("INSERT INTO podcast_episodes(id,subscription_id,user_id,recording_id,identity_kind,identity_value,enclosure_url,declared_mime,byte_length,description,published_at) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(subscription_id,identity_kind,identity_value) DO UPDATE SET enclosure_revision=podcast_episodes.enclosure_revision+CASE WHEN podcast_episodes.enclosure_url IS NOT excluded.enclosure_url OR podcast_episodes.declared_mime IS NOT excluded.declared_mime OR podcast_episodes.byte_length IS NOT excluded.byte_length THEN 1 ELSE 0 END,enclosure_url=excluded.enclosure_url,declared_mime=excluded.declared_mime,byte_length=excluded.byte_length,description=excluded.description,published_at=excluded.published_at,last_seen_at=unixepoch() RETURNING id")
            .bind(Uuid::new_v4().to_string()).bind(&lease.subscription_id).bind(&lease.user_id).bind(&recording).bind(kind).bind(value)
            .bind(episode.enclosure_url).bind(episode.declared_mime).bind(length).bind(episode.description).bind(episode.published_at).fetch_one(&mut **transaction).await?;
        let source: Option<String> =
            sqlx::query_scalar("SELECT source_id FROM podcast_sources WHERE episode_id=?")
                .bind(&episode_id)
                .fetch_optional(&mut **transaction)
                .await?;
        if let Some(source) = source {
            let retained = sqlx::query("UPDATE sources SET ready=1 WHERE id=? AND recording_id=? AND provider='podcast' AND provider_id=? AND device_id IS NULL")
                .bind(source).bind(&recording).bind(&episode_id).execute(&mut **transaction).await?;
            if retained.rows_affected() != 1 {
                return Err(Error::Internal("Podcast source identity changed".into()));
            }
        } else {
            let source = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,device_id,ready) VALUES(?,?,'podcast',?,NULL,1)")
                .bind(&source).bind(&recording).bind(&episode_id).execute(&mut **transaction).await?;
            sqlx::query("INSERT INTO podcast_sources(episode_id,source_id) VALUES(?,?)")
                .bind(&episode_id)
                .bind(source)
                .execute(&mut **transaction)
                .await?;
        }
    }
    sqlx::query("UPDATE podcast_subscriptions SET title=?,author=?,description=?,content_revision=content_revision+1,last_refreshed_at=unixepoch(),next_refresh_at=unixepoch()+3600,error=NULL WHERE id=? AND generation=?")
        .bind(snapshot.title).bind(snapshot.author).bind(snapshot.description).bind(&lease.subscription_id).bind(lease.generation).execute(&mut **transaction).await?;
    Ok(())
}

fn stored_number(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::Upstream("The podcast feed contains an oversized episode number"))
}

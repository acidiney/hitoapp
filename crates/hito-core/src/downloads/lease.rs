use super::{Downloads, origin::DownloadOrigin};
use crate::{Error, Result, Store, library_storage::ManagedAttempt};
use sqlx::SqliteConnection;
use std::time::Duration;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(super) struct Lease {
    pub id: String,
    pub account_id: String,
    pub intent_id: String,
    pub recording_id: String,
    pub host_device: String,
    pub generation: i64,
    pub attempts: i64,
    pub lease_token: String,
    pub file_id: String,
    #[sqlx(flatten)]
    pub origin: DownloadOrigin,
    #[sqlx(skip)]
    pub active: Option<super::active::ActiveJob>,
    #[sqlx(skip)]
    pub reservation: Option<ManagedAttempt>,
}

impl Downloads {
    pub(super) async fn claim(&self) -> Result<Option<Lease>> {
        let eligible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM download_jobs j INDEXED BY download_job_queue
            JOIN download_intents i ON i.id=j.intent_id WHERE j.status='queued' AND j.next_attempt_at<=unixepoch() AND i.host_device=?)
            OR EXISTS(SELECT 1 FROM download_jobs j INDEXED BY download_job_expired JOIN download_intents i ON i.id=j.intent_id
                WHERE j.lease_token IS NOT NULL AND j.status='running' AND j.lease_until<=unixepoch() AND i.host_device=?)")
            .bind(self.host.as_str()).bind(self.host.as_str()).fetch_one(&self.store.pool).await?;
        if !eligible {
            return Ok(None);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        self.recover_expired_on(&mut tx).await?;
        sqlx::query("UPDATE download_intents INDEXED BY download_intent_queued_host SET state='failed',error='authority_changed',updated_at=unixepoch()
            WHERE host_device=? AND state='queued' AND NOT EXISTS(SELECT 1 FROM current_download_authority a WHERE a.id=download_intents.id)")
            .bind(self.host.as_str()).execute(&mut *tx).await?;
        sqlx::query("UPDATE download_jobs INDEXED BY download_job_queue SET status='cancelled',updated_at=unixepoch() WHERE status='queued'
            AND EXISTS(SELECT 1 FROM download_intents i WHERE i.id=download_jobs.intent_id AND i.host_device=? AND i.state IN ('failed','removed'))")
            .bind(self.host.as_str()).execute(&mut *tx).await?;
        let id: Option<String> = sqlx::query_scalar("SELECT j.id FROM download_jobs j JOIN download_intents i ON i.id=j.intent_id
            JOIN current_download_authority a ON a.id=i.id WHERE i.host_device=? AND j.status='queued' AND j.attempts<3 AND j.next_attempt_at<=unixepoch()
            AND NOT EXISTS(SELECT 1 FROM download_jobs busy WHERE busy.account_id=j.account_id AND busy.status='running')
            AND (SELECT count(*) FROM download_jobs busy WHERE busy.status='running')<2 ORDER BY j.created_at,j.id LIMIT 1")
            .bind(self.host.as_str()).fetch_optional(&mut *tx).await?;
        let Some(id) = id else {
            tx.commit().await?;
            return Ok(None);
        };
        let token = Uuid::new_v4().to_string();
        let file = Uuid::new_v4().to_string();
        sqlx::query("UPDATE download_jobs SET status='running',attempts=attempts+1,lease_token=?,lease_until=unixepoch()+30,updated_at=unixepoch() WHERE id=?")
            .bind(&token).bind(&id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO download_files(id,account_id,host_device,recording_id,state,attempt_id)
            SELECT ?,i.account_id,i.host_device,i.recording_id,'staging',? FROM download_intents i JOIN download_jobs j ON j.intent_id=i.id WHERE j.id=?")
            .bind(&file).bind(&token).bind(&id).execute(&mut *tx).await?;
        sqlx::query("UPDATE download_intents SET state='running',file_id=?,bytes_received=0,total_bytes=NULL,error=NULL,updated_at=unixepoch()
            WHERE id=(SELECT intent_id FROM download_jobs WHERE id=?)")
            .bind(&file).bind(&id).execute(&mut *tx).await?;
        let mut lease: Lease = sqlx::query_as("SELECT j.id,j.account_id,j.intent_id,j.generation,j.attempts,j.lease_token,
            i.recording_id,i.host_device,i.file_id,i.provider_source_id,i.binding_id,i.binding_revision,i.remote_source_id,
            i.installation_id,i.installation_revision,i.service_id,i.service_revision,i.credential_digest,i.audio_generation,
            i.origin_kind,i.podcast_source_id,i.podcast_episode_id,i.podcast_subscription_id,
            i.podcast_subscription_generation,i.podcast_enclosure_revision,i.podcast_enclosure_url
            FROM download_jobs j JOIN download_intents i ON i.id=j.intent_id WHERE j.id=?")
            .bind(&id).fetch_one(&mut *tx).await?;
        lease.active = Some(self.active.enter(&lease.lease_token)?);
        lease.reservation = Some(self.files.reserve(
            stored_uuid(&lease.file_id)?,
            stored_uuid(&lease.lease_token)?,
        )?);
        tx.commit().await?;
        tracing::debug!(job_id = %lease.id, intent_id = %lease.intent_id, account_id = %lease.account_id, attempt = lease.attempts, "Download lease acquired");
        Ok(Some(lease))
    }

    async fn recover_expired_on(&self, connection: &mut SqliteConnection) -> Result<()> {
        let expired: Vec<(String, String, Option<String>, String)> = sqlx::query_as("SELECT j.id,j.intent_id,i.file_id,j.lease_token
            FROM download_jobs j JOIN download_intents i ON i.id=j.intent_id WHERE i.host_device=?
            AND j.status='running' AND j.lease_until<=unixepoch() ORDER BY j.lease_until,j.id LIMIT 32")
            .bind(self.host.as_str()).fetch_all(&mut *connection).await?;
        for (job, intent, file, token) in expired {
            let managed = match &file {
                Some(file) => self
                    .files
                    .active(stored_uuid(file)?, stored_uuid(&token)?)?,
                None => false,
            };
            if self.active.contains(&token)? || managed {
                continue;
            }
            if let Some(file) = file {
                self.files
                    .revoke(stored_uuid(&file)?, stored_uuid(&token)?)?;
                sqlx::query("UPDATE download_files SET state='deleting',cleanup_after=0 WHERE id=? AND state='staging'")
                    .bind(file).execute(&mut *connection).await?;
            }
            sqlx::query("UPDATE download_intents SET state=CASE WHEN (SELECT attempts FROM download_jobs WHERE id=?)>=3 THEN 'failed' ELSE 'queued' END,
                file_id=NULL,error='lease_expired',updated_at=unixepoch() WHERE id=? AND state IN ('running','verifying')")
                .bind(&job).bind(&intent).execute(&mut *connection).await?;
            sqlx::query("UPDATE download_jobs SET status=CASE WHEN (SELECT state FROM download_intents WHERE id=?)='removed' THEN 'cancelled'
                WHEN attempts>=3 THEN 'failed' ELSE 'queued' END,lease_token=NULL,lease_until=NULL,updated_at=unixepoch() WHERE id=? AND lease_token=?")
                .bind(&intent).bind(&job).bind(&token).execute(&mut *connection).await?;
            tracing::debug!(job_id = %job, outcome = "recovered", "Expired download lease recovered");
        }
        Ok(())
    }
}

impl Lease {
    pub(super) fn authority(&self, store: &Store) -> DownloadAuthority {
        DownloadAuthority {
            store: store.clone(),
            intent: self.intent_id.clone(),
            token: self.lease_token.clone(),
        }
    }

    pub async fn check(&self, store: &Store) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(1), async {
            self.check_on(&mut *store.pool.acquire().await?).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Download authority could not be checked in time",
        )))
    }

    pub async fn check_on(&self, connection: &mut SqliteConnection) -> Result<()> {
        let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM download_jobs j
            JOIN download_intents i ON i.id=j.intent_id AND i.generation=j.generation
            JOIN current_download_authority authority ON authority.id=i.id
            JOIN download_files f ON f.id=i.file_id AND f.state='staging' AND f.attempt_id=j.lease_token
                AND f.account_id=i.account_id AND f.recording_id=i.recording_id AND f.host_device=i.host_device
            WHERE j.id=? AND j.account_id=? AND j.intent_id=? AND j.generation=? AND j.status='running'
                AND j.lease_token=? AND j.lease_until>unixepoch() AND i.file_id=? AND i.host_device=?)")
            .bind(&self.id).bind(&self.account_id).bind(&self.intent_id).bind(self.generation)
            .bind(&self.lease_token).bind(&self.file_id).bind(&self.host_device).fetch_one(connection).await?;
        if current {
            Ok(())
        } else {
            Err(Error::Conflict("This download is no longer current".into()))
        }
    }

    pub async fn heartbeat(&self, store: &Store, bytes: u64, total: Option<u64>) -> Result<()> {
        let mut tx = store.pool.begin_with("BEGIN IMMEDIATE").await?;
        self.renew_on(&mut tx).await?;
        sqlx::query("UPDATE download_intents SET bytes_received=?,total_bytes=?,updated_at=unixepoch() WHERE id=? AND generation=?")
            .bind(bytes as i64).bind(total.map(|value| value as i64)).bind(&self.intent_id).bind(self.generation).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn retire(&self, store: &Store) -> Result<()> {
        let retirement = self.reservation()?.retire();
        tokio::pin!(retirement);
        let period = Duration::from_secs(1);
        let mut renewal = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        renewal.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut current = true;
        loop {
            tokio::select! {
                biased;
                result = &mut retirement => return result,
                _ = renewal.tick(), if current => {
                    if let Err(error) = self.renew_retiring(store).await {
                        current = false;
                        tracing::debug!(job_id = %self.id, error_code = error.code(), "Download retirement lease renewal stopped");
                    }
                }
            }
        }
    }

    async fn renew_retiring(&self, store: &Store) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(1), async {
            let mut tx = store.pool.begin_with("BEGIN IMMEDIATE").await?;
            self.renew_on(&mut tx).await?;
            tx.commit().await?;
            tracing::debug!(job_id = %self.id, "Download retirement lease renewed");
            Ok(())
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Download lease could not be renewed in time",
        )))
    }

    async fn renew_on(&self, connection: &mut SqliteConnection) -> Result<()> {
        self.check_on(connection).await?;
        sqlx::query("UPDATE download_jobs SET lease_until=unixepoch()+30,updated_at=unixepoch() WHERE id=? AND lease_token=?")
            .bind(&self.id).bind(&self.lease_token).execute(connection).await?;
        Ok(())
    }

    pub fn reservation(&self) -> Result<&ManagedAttempt> {
        self.reservation
            .as_ref()
            .ok_or_else(|| Error::Internal("Download attempt ownership is unavailable".into()))
    }
}

#[derive(Clone)]
pub(crate) struct DownloadAuthority {
    store: Store,
    intent: String,
    token: String,
}

impl DownloadAuthority {
    pub(crate) async fn check(&self) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(1), async {
            let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM download_jobs j
                JOIN download_intents i ON i.id=j.intent_id AND i.generation=j.generation
                JOIN current_download_authority authority ON authority.id=i.id
                JOIN download_files f ON f.id=i.file_id AND f.state='staging' AND f.attempt_id=j.lease_token
                    AND f.account_id=i.account_id AND f.recording_id=i.recording_id AND f.host_device=i.host_device
                WHERE j.intent_id=? AND j.lease_token=? AND j.status='running' AND j.lease_until>unixepoch())")
                .bind(&self.intent).bind(&self.token).fetch_one(&self.store.pool).await?;
            if current {
                Ok(())
            } else {
                Err(Error::Conflict("This download is no longer current".into()))
            }
        })
        .await
        .unwrap_or(Err(Error::Upstream("Download authority could not be checked in time")))
    }
}

pub(super) fn stored_uuid(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|_| Error::Internal("Stored download identity is invalid".into()))
}

use super::{Downloads, lease::Lease};
use crate::{Result, library_storage::ManagedFileLocation};
use uuid::Uuid;

const STORAGE_RETRY_SECONDS: i64 = 120;

impl Downloads {
    pub(super) async fn mark_verifying(&self, lease: &Lease) -> Result<()> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        lease.check_on(&mut tx).await?;
        sqlx::query("UPDATE download_intents SET state='verifying',updated_at=unixepoch() WHERE id=? AND generation=?")
            .bind(&lease.intent_id).bind(lease.generation).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn record_verified(
        &self,
        lease: &Lease,
        location: &ManagedFileLocation,
    ) -> Result<()> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        lease.check_on(&mut tx).await?;
        sqlx::query("UPDATE download_files SET checksum=?,byte_length=?,mime=? WHERE id=? AND state='staging' AND attempt_id=?")
            .bind(&location.checksum).bind(location.byte_length as i64).bind(location.mime)
            .bind(&lease.file_id).bind(&lease.lease_token).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn publish_ready(
        &self,
        lease: &Lease,
        location: &ManagedFileLocation,
    ) -> Result<()> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        lease.check_on(&mut tx).await?;
        let source = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,device_id,ready) VALUES(?,?,'download',?,?,1)")
            .bind(&source).bind(&lease.recording_id).bind(&lease.file_id).bind(&lease.host_device).execute(&mut *tx).await?;
        sqlx::query("UPDATE download_files SET state='ready',source_id=?,checksum=?,byte_length=?,mime=? WHERE id=? AND state='staging' AND attempt_id=?")
            .bind(&source).bind(&location.checksum).bind(location.byte_length as i64).bind(location.mime)
            .bind(&lease.file_id).bind(&lease.lease_token).execute(&mut *tx).await?;
        sqlx::query("UPDATE download_intents SET state='ready',bytes_received=?,total_bytes=?,error=NULL,updated_at=unixepoch() WHERE id=? AND generation=?")
            .bind(location.byte_length as i64).bind(location.byte_length as i64).bind(&lease.intent_id).bind(lease.generation).execute(&mut *tx).await?;
        sqlx::query("UPDATE download_jobs SET status='complete',lease_token=NULL,lease_until=NULL,updated_at=unixepoch() WHERE id=? AND lease_token=?")
            .bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
        tx.commit().await?;
        tracing::debug!(source_id = %source, file_id = %lease.file_id, "Verified download source published");
        Ok(())
    }

    pub(super) async fn finish_failed(
        &self,
        lease: &Lease,
        error: Option<crate::Error>,
        stopped: bool,
    ) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM download_jobs WHERE id=? AND lease_token=? AND status='running')")
            .bind(&lease.id).bind(&lease.lease_token).fetch_one(&mut *tx).await?;
        let current = if owned {
            lease.check_on(&mut tx).await.is_ok()
        } else {
            false
        };
        let storage_full =
            current && !stopped && matches!(&error, Some(crate::Error::InsufficientStorage));
        let (state, job, reason, refund, delay) = if !current {
            ("failed", "cancelled", Some("authority_changed"), false, 0)
        } else if stopped {
            ("queued", "queued", None, true, 0)
        } else if storage_full {
            ("queued", "queued", None, true, STORAGE_RETRY_SECONDS)
        } else if lease.attempts < 3 && matches!(&error, Some(crate::Error::Upstream(_))) {
            (
                "queued",
                "queued",
                Some("provider_unavailable"),
                false,
                1i64 << lease.attempts,
            )
        } else {
            ("failed", "failed", Some("transfer_failed"), false, 0)
        };
        if let Some(error) = error {
            tracing::debug!(job_id = %lease.id, error_code = error.code(), "Download attempt did not publish");
        }
        if owned {
            sqlx::query("UPDATE download_jobs SET status=?,attempts=max(attempts-?,0),next_attempt_at=unixepoch()+?,lease_token=NULL,lease_until=NULL,updated_at=unixepoch() WHERE id=? AND lease_token=?")
                .bind(job).bind(i64::from(refund)).bind(delay).bind(&lease.id).bind(&lease.lease_token).execute(&mut *tx).await?;
            sqlx::query("UPDATE download_intents SET state=?,file_id=NULL,error=?,updated_at=unixepoch() WHERE id=? AND generation=? AND state!='removed'")
                .bind(state).bind(reason).bind(&lease.intent_id).bind(lease.generation).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE download_files SET state='deleting',cleanup_after=0 WHERE id=? AND attempt_id=? AND state!='ready'")
            .bind(&lease.file_id).bind(&lease.lease_token).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(job)
    }
}

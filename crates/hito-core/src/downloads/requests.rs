use super::{DownloadReceipt, DownloadRequest, Downloads};
use crate::{Actor, Error, Result};
use sha2::{Digest, Sha256};
use uuid::Uuid;

impl Downloads {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn create(
        &self,
        actor: &Actor,
        recording: &str,
        request: DownloadRequest,
    ) -> Result<DownloadReceipt> {
        let recording = identity(recording)?;
        let digest = format!(
            "{:x}",
            Sha256::digest(format!(
                "hito.download.track.v1|{recording}|{}",
                self.host.as_str()
            ))
        );
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let previous: Option<(String, String)> = sqlx::query_as("SELECT request_digest,intent_id FROM download_receipts WHERE account_id=? AND request_id=?")
            .bind(actor.id()).bind(request.request_id.to_string()).fetch_optional(&mut *tx).await?;
        if let Some((previous_digest, intent)) = previous {
            if previous_digest != digest {
                return Err(Error::Conflict(
                    "This download request was already used for a different recording or host"
                        .into(),
                ));
            }
            tx.commit().await?;
            tracing::info!(intent_id = %intent, outcome = "replayed", "Download request retained");
            return Ok(DownloadReceipt {
                request_id: request.request_id,
                intent_id: intent,
                recording_id: recording,
            });
        }
        self.files.ensure_supported()?;
        let (library, grant): (String, String) = sqlx::query_as(
            "SELECT r.library_id,g.id FROM recordings r
            JOIN library_access_grants g ON g.library_id=r.library_id AND g.user_id=? WHERE r.id=?",
        )
        .bind(actor.id())
        .bind(&recording)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        let existing = self
            .retain_existing_on(&mut tx, actor.id(), &recording, &grant)
            .await?;
        let intent = if let Some(intent) = existing {
            intent
        } else {
            let (account_pending, pending): (i64, i64) = sqlx::query_as("SELECT coalesce(sum(account_id=?),0),count(*) FROM download_jobs WHERE status IN ('queued','running')")
                .bind(actor.id()).fetch_one(&mut *tx).await?;
            if account_pending >= 32 || pending >= 256 {
                return Err(Error::Conflict("Download work is busy. Try again.".into()));
            }
            let origin = self
                .plugins
                .capture_download_origin_on(&mut tx, actor.id(), &recording)
                .await?;
            let intent = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO download_intents(id,account_id,recording_id,library_id,access_grant_id,host_device,state,
                provider_source_id,binding_id,binding_revision,remote_source_id,installation_id,installation_revision,service_id,service_revision,credential_digest,audio_generation)
                VALUES(?,?,?,?,?,?,'queued',?,?,?,?,?,?,?,?,?,?)")
                .bind(&intent).bind(actor.id()).bind(&recording).bind(library).bind(grant).bind(self.host.as_str())
                .bind(origin.provider_source_id).bind(origin.binding_id).bind(origin.binding_revision).bind(origin.remote_source_id)
                .bind(origin.installation_id).bind(origin.installation_revision).bind(origin.service_id).bind(origin.service_revision)
                .bind(origin.credential_digest).bind(origin.audio_generation).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO download_jobs(id,account_id,intent_id,generation,status) VALUES(?,?,?,1,'queued')")
                .bind(Uuid::new_v4().to_string()).bind(actor.id()).bind(&intent).execute(&mut *tx).await?;
            intent
        };
        sqlx::query("INSERT INTO download_receipts(account_id,request_id,request_digest,intent_id) VALUES(?,?,?,?)")
            .bind(actor.id()).bind(request.request_id.to_string()).bind(digest).bind(&intent).execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::info!(intent_id = %intent, outcome = "retained", "Download requested");
        Ok(DownloadReceipt {
            request_id: request.request_id,
            intent_id: intent,
            recording_id: recording,
        })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn remove(&self, actor: &Actor, intent: &str) -> Result<()> {
        let intent = identity(intent)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let file: (Option<String>, Option<String>) = sqlx::query_as("SELECT f.id,f.attempt_id FROM download_intents i
            LEFT JOIN download_files f ON f.id=i.file_id WHERE i.id=? AND i.account_id=? AND i.host_device=?")
            .bind(&intent).bind(actor.id()).bind(self.host.as_str()).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        sqlx::query("UPDATE download_intents SET state='removed',generation=generation+1,error=NULL,updated_at=unixepoch() WHERE id=? AND state!='removed'")
            .bind(&intent).execute(&mut *tx).await?;
        sqlx::query("UPDATE download_jobs SET status='cancelled',updated_at=unixepoch() WHERE intent_id=? AND status='queued'")
            .bind(&intent).execute(&mut *tx).await?;
        sqlx::query("UPDATE download_files SET state='deleting' WHERE host_device=? AND state IN ('staging','ready')
            AND id IN (SELECT file_id FROM download_intents WHERE id=?)
            AND NOT EXISTS(SELECT 1 FROM download_intents reference WHERE reference.file_id=download_files.id AND reference.state!='removed')")
            .bind(self.host.as_str()).bind(&intent).execute(&mut *tx).await?;
        tx.commit().await?;
        if let (Some(file), Some(attempt)) = file {
            self.files.revoke(
                super::lease::stored_uuid(&file)?,
                super::lease::stored_uuid(&attempt)?,
            )?;
            if let Err(error) = self.reclaim_file(&file).await {
                tracing::debug!(
                    error_code = error.code(),
                    outcome = "cleanup_pending",
                    "Download removal cleanup deferred"
                );
            }
        }
        tracing::info!(intent_id = %intent, outcome = "removed", "Download reference removed");
        Ok(())
    }
}

fn identity(value: &str) -> Result<String> {
    Uuid::parse_str(value)
        .map(|id| id.to_string())
        .map_err(|_| Error::NotFound)
}

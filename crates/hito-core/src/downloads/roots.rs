use super::{Downloads, lease::stored_uuid};
use crate::{Actor, Error, Result};
use sqlx::SqliteConnection;
use uuid::Uuid;

pub(super) async fn retain_recording_on(
    connection: &mut SqliteConnection,
    account: &str,
    host: &str,
    recording: &str,
    grant: &str,
    transfer: &str,
) -> Result<String> {
    let previous: Option<(String, String, String)> = sqlx::query_as("SELECT id,access_grant_id,transfer_id FROM download_retentions
        WHERE account_id=? AND recording_id=? AND host_device=? AND kind='recording' AND state='active'")
        .bind(account).bind(recording).bind(host).fetch_optional(&mut *connection).await?;
    if let Some((root, previous_grant, previous_transfer)) = previous {
        if previous_grant == grant && previous_transfer == transfer {
            return Ok(root);
        }
        sqlx::query(
            "UPDATE download_retentions SET state='removed',updated_at=unixepoch() WHERE id=?",
        )
        .bind(root)
        .execute(&mut *connection)
        .await?;
    }
    let root = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO download_retentions(id,account_id,host_device,kind,recording_id,access_grant_id,transfer_id,state)
        VALUES(?,?,?,'recording',?,?,?,'active')")
        .bind(&root).bind(account).bind(host).bind(recording).bind(grant).bind(transfer).execute(connection).await?;
    Ok(root)
}

impl Downloads {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn remove(&self, actor: &Actor, retention: &str) -> Result<()> {
        let started = std::time::Instant::now();
        let retention = identity(retention)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let (recording, playlist): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT recording_id,playlist_id FROM download_retentions
            WHERE id=? AND account_id=? AND host_device=?",
        )
        .bind(&retention)
        .bind(actor.id())
        .bind(self.host.as_str())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        let changed = sqlx::query("UPDATE download_retentions SET state='removed',updated_at=unixepoch() WHERE id=? AND state='active'")
            .bind(&retention).execute(&mut *tx).await?.rows_affected();
        let retired = if changed > 0 {
            if let Some(recording) = recording {
                self.retire_unreferenced_on(&mut tx, actor.id(), &recording)
                    .await?
            } else {
                sqlx::query("UPDATE download_playlist_work SET status='queued',phase='prune',cursor=NULL,next_attempt_at=0 WHERE retention_id=?")
                    .bind(&retention).execute(&mut *tx).await?;
                None
            }
        } else {
            None
        };
        tx.commit().await?;
        if let Some((file, attempt)) = retired {
            self.files
                .revoke(stored_uuid(&file)?, stored_uuid(&attempt)?)?;
            if let Err(error) = self.reclaim_file(&file).await {
                tracing::debug!(
                    error_code = error.code(),
                    outcome = "cleanup_pending",
                    "Download removal cleanup deferred"
                );
            }
        }
        tracing::info!(retention_id = %retention, playlist = playlist.is_some(), outcome = "removed", elapsed_ms = started.elapsed().as_millis() as u64, "Download retention removed");
        Ok(())
    }

    pub(super) async fn retire_unreferenced_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        recording: &str,
    ) -> Result<Option<(String, String)>> {
        let selected: Option<(String, Option<String>, Option<String>, bool)> = sqlx::query_as("SELECT i.id,f.id,f.attempt_id,
            EXISTS(SELECT 1 FROM retained_download_intents retained WHERE retained.id=i.id) FROM download_intents i
            LEFT JOIN download_files f ON f.id=i.file_id WHERE i.account_id=? AND i.recording_id=? AND i.host_device=? AND i.state!='removed'
            ")
            .bind(account).bind(recording).bind(self.host.as_str()).fetch_optional(&mut *connection).await?;
        let Some((intent, file, attempt, retained)) = selected else {
            return Ok(None);
        };
        if retained {
            if file.is_some() {
                self.retain_retirement_candidate_on(connection, &intent)
                    .await?;
            } else {
                sqlx::query("DELETE FROM download_retirement_work WHERE transfer_id=?")
                    .bind(&intent)
                    .execute(connection)
                    .await?;
            }
            return Ok(None);
        }
        sqlx::query("DELETE FROM download_retirement_work WHERE transfer_id=?")
            .bind(&intent)
            .execute(&mut *connection)
            .await?;
        sqlx::query("UPDATE download_intents SET state='removed',generation=generation+1,error=NULL,updated_at=unixepoch() WHERE id=? AND state!='removed'")
            .bind(&intent).execute(&mut *connection).await?;
        sqlx::query("UPDATE download_jobs SET status='cancelled',updated_at=unixepoch() WHERE intent_id=? AND status='queued'")
            .bind(&intent).execute(&mut *connection).await?;
        sqlx::query("UPDATE download_files SET state='deleting',cleanup_after=0 WHERE id=? AND state IN ('staging','ready')
            AND NOT EXISTS(SELECT 1 FROM download_intents reference WHERE reference.file_id=download_files.id AND reference.state!='removed')")
            .bind(&file).execute(connection).await?;
        tracing::debug!(transfer_id = %intent, outcome = "unreferenced", "Shared download transfer retired");
        Ok(file.zip(attempt))
    }
}

pub(super) fn identity(value: &str) -> Result<String> {
    Uuid::parse_str(value)
        .map(|id| id.to_string())
        .map_err(|_| Error::NotFound)
}

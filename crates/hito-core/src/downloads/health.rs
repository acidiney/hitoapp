use super::{DownloadState, DownloadStatus, Downloads, lease::stored_uuid};
use crate::{
    Actor, Error, Result, Store,
    library_storage::{ManagedFileLocation, managed_mime},
};
use sqlx::{Row, sqlite::SqliteRow};

#[derive(PartialEq, Eq)]
struct ReadyCopy {
    file: ManagedFileLocation,
    intent_id: String,
    intent_generation: i64,
    access_grant: String,
}

impl Downloads {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn status(&self, actor: &Actor, recording: &str) -> Result<DownloadStatus> {
        let recording = uuid::Uuid::parse_str(recording)
            .map_err(|_| Error::NotFound)?
            .to_string();
        for _ in 0..3 {
            let (status, copy) = self.read_status(actor, &recording).await?;
            let Some(copy) = copy else {
                return Ok(status);
            };
            if !self.files.available(&copy.file).await? {
                retire_unusable_copy(&self.store, &copy.file).await?;
                continue;
            }
            let (current, current_copy) = self.read_status(actor, &recording).await?;
            if current_copy.as_ref() == Some(&copy) {
                return Ok(current);
            }
            if current_copy.is_none() {
                return Ok(current);
            }
        }
        Err(Error::Conflict("Download state changed. Try again.".into()))
    }

    async fn read_status(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<(DownloadStatus, Option<ReadyCopy>)> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let row = sqlx::query("SELECT i.id,i.state,i.generation AS intent_generation,i.access_grant_id,i.bytes_received,i.total_bytes,i.error,
            f.source_id,f.id AS file_id,f.attempt_id,f.generation AS file_generation,f.checksum,f.byte_length,f.mime
            FROM download_intents i JOIN recordings r ON r.id=i.recording_id AND r.library_id=i.library_id
            JOIN library_access_grants g ON g.library_id=i.library_id AND g.user_id=i.account_id AND g.id=i.access_grant_id
            LEFT JOIN download_files f ON f.id=i.file_id AND f.account_id=i.account_id AND f.host_device=i.host_device
                AND f.recording_id=i.recording_id AND f.state='ready'
            WHERE i.account_id=? AND i.recording_id=? AND i.host_device=? AND i.state!='removed'")
            .bind(actor.id()).bind(recording).bind(self.host.as_str()).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        let status = match row.try_get::<&str, _>("state")? {
            "queued" => DownloadState::Queued,
            "running" => DownloadState::Running,
            "verifying" => DownloadState::Verifying,
            "ready" => DownloadState::Ready,
            _ => DownloadState::Failed,
        };
        let result = DownloadStatus {
            status,
            intent_id: row.try_get("id")?,
            recording_id: recording.into(),
            source_id: row.try_get("source_id")?,
            host_device_id: self.host.as_str().into(),
            bytes_received: row.try_get::<i64, _>("bytes_received")? as u64,
            total_bytes: row
                .try_get::<Option<i64>, _>("total_bytes")?
                .map(|bytes| bytes as u64),
            error: row.try_get("error")?,
        };
        let copy = if matches!(status, DownloadState::Ready) {
            Some(ready_copy(&row, &result.intent_id)?)
        } else {
            None
        };
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        Ok((result, copy))
    }
}

fn ready_copy(row: &SqliteRow, intent: &str) -> Result<ReadyCopy> {
    let mime: String = row.try_get("mime")?;
    let byte_length =
        u64::try_from(row.try_get::<i64, _>("byte_length")?).map_err(|_| Error::NotFound)?;
    Ok(ReadyCopy {
        file: ManagedFileLocation {
            file_id: stored_uuid(row.try_get("file_id")?)?,
            attempt_id: stored_uuid(row.try_get("attempt_id")?)?,
            generation: row.try_get("file_generation")?,
            checksum: row.try_get("checksum")?,
            byte_length,
            mime: managed_mime(&mime).ok_or(Error::NotFound)?,
        },
        intent_id: intent.into(),
        intent_generation: row.try_get("intent_generation")?,
        access_grant: row.try_get("access_grant_id")?,
    })
}

pub(crate) async fn retire_unusable_copy(store: &Store, file: &ManagedFileLocation) -> Result<()> {
    let mut tx = store.pool.begin_with("BEGIN IMMEDIATE").await?;
    let changed = sqlx::query("UPDATE download_files SET state='deleting',cleanup_after=0
        WHERE id=? AND attempt_id=? AND generation=? AND checksum=? AND byte_length=? AND mime=? AND state='ready'")
        .bind(file.file_id.to_string()).bind(file.attempt_id.to_string()).bind(file.generation)
        .bind(&file.checksum).bind(file.byte_length as i64).bind(file.mime).execute(&mut *tx).await?.rows_affected();
    if changed == 1 {
        sqlx::query("UPDATE download_intents SET state='failed',file_id=NULL,error='file_unavailable',updated_at=unixepoch()
            WHERE file_id=? AND state='ready'")
            .bind(file.file_id.to_string()).execute(&mut *tx).await?;
        sqlx::query(
            "UPDATE sources SET ready=0 WHERE provider='download' AND provider_id=?
            AND id=(SELECT source_id FROM download_files WHERE id=?)",
        )
        .bind(file.file_id.to_string())
        .bind(file.file_id.to_string())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    if changed == 1 {
        tracing::info!(file_id = %file.file_id, outcome = "file_unavailable", "Unusable retained download fenced");
    }
    Ok(())
}

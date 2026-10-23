use super::{DownloadReceipt, DownloadRequest, Downloads, roots};
use crate::{Actor, Error, Result};
use sha2::{Digest, Sha256};

impl Downloads {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn create(
        &self,
        actor: &Actor,
        recording: &str,
        request: DownloadRequest,
    ) -> Result<DownloadReceipt> {
        let started = std::time::Instant::now();
        let recording = roots::identity(recording)?;
        let digest = format!(
            "{:x}",
            Sha256::digest(format!(
                "hito.download.track.v1|{recording}|{}",
                self.host.as_str()
            ))
        );
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let previous: Option<(String, String)> = sqlx::query_as("SELECT request_digest,coalesce(retention_id,intent_id) FROM download_receipts WHERE account_id=? AND request_id=?")
            .bind(actor.id()).bind(request.request_id.to_string()).fetch_optional(&mut *tx).await?;
        if let Some((previous_digest, retention)) = previous {
            if previous_digest != digest {
                return Err(Error::Conflict(
                    "This download request was already used for a different recording or host"
                        .into(),
                ));
            }
            tx.commit().await?;
            tracing::info!(retention_id = %retention, outcome = "replayed", "Download request retained");
            return Ok(DownloadReceipt {
                request_id: request.request_id,
                intent_id: retention,
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
        let transfer = self
            .ensure_transfer_on(&mut tx, actor.id(), &recording, &library, &grant)
            .await?;
        let retention = roots::retain_recording_on(
            &mut tx,
            actor.id(),
            self.host.as_str(),
            &recording,
            &grant,
            &transfer,
        )
        .await?;
        sqlx::query("INSERT INTO download_receipts(account_id,request_id,request_digest,intent_id,retention_id) VALUES(?,?,?,?,?)")
            .bind(actor.id()).bind(request.request_id.to_string()).bind(digest).bind(&transfer).bind(&retention).execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::info!(retention_id = %retention, transfer_id = %transfer, outcome = "retained", elapsed_ms = started.elapsed().as_millis() as u64, "Download requested");
        Ok(DownloadReceipt {
            request_id: request.request_id,
            intent_id: retention,
            recording_id: recording,
        })
    }
}

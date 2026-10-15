use super::Indexer;
use crate::Result;

impl Indexer {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(host_device = self.host.as_str()))]
    pub async fn run_next_job(&self) -> Result<bool> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT id, library_id FROM jobs
             WHERE state = 'queued' OR (state = 'running' AND lease_until < unixepoch())
             ORDER BY created_at, id LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some((id, library)) = row else {
            return Ok(false);
        };
        sqlx::query(
            "UPDATE jobs SET state = 'running', lease_until = unixepoch() + 300, error = NULL
             WHERE id = ?",
        )
        .bind(&id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::info!(job_id = %id, "Scan lease acquired");
        match self.scan(&id, &library).await {
            Ok(count) => {
                sqlx::query(
                    "UPDATE jobs SET state = 'complete', progress = ?, lease_until = NULL WHERE id = ?",
                ).bind(count).bind(&id).execute(&self.store.pool).await?;
                tracing::info!(job_id = %id, files_read = count, "Scan completed");
            }
            Err(error) => {
                tracing::warn!(job_id = %id, error_code = error.code(), "Scan failed");
                sqlx::query(
                    "UPDATE jobs SET state = 'failed',
                         error = 'Music scan failed. Check the music folder and installed FFmpeg tools.',
                         lease_until = NULL
                     WHERE id = ?",
                ).bind(&id).execute(&self.store.pool).await?;
            }
        }
        Ok(true)
    }
}

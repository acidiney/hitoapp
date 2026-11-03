use super::{Downloads, lease::stored_uuid};
use crate::Result;
use sqlx::QueryBuilder;
use std::{collections::HashSet, time::Instant};

impl Downloads {
    pub(crate) async fn sweep_staging(&self) -> Result<()> {
        let started = Instant::now();
        let found = self.files.staging_files().await?;
        let live: HashSet<String> = sqlx::query_scalar(
            "SELECT lease_token FROM download_jobs WHERE status='running' AND lease_until>unixepoch()",
        )
        .fetch_all(&self.store.pool)
        .await?
        .into_iter()
        .collect();
        let found_count = found.len();
        let stale: Vec<_> = found
            .into_iter()
            .filter(|(_, attempt)| !live.contains(&attempt.to_string()))
            .collect();
        let kept_live = found_count - stale.len();
        let removed = self.files.remove_staging(stale).await?;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if removed > 0 {
            tracing::info!(
                event = "download_staging_swept",
                removed,
                kept_live,
                elapsed_ms,
                "Leftover download staging files removed"
            );
        } else {
            tracing::debug!(
                event = "download_staging_swept",
                removed,
                kept_live,
                elapsed_ms,
                "No leftover download staging files"
            );
        }
        Ok(())
    }

    pub(super) async fn reclaim_pending(&self) -> Result<bool> {
        let erased = match self.reclaim_erased().await {
            Ok(erased) => erased,
            Err(error) => {
                tracing::debug!(
                    error_code = error.code(),
                    "Erased download cleanup deferred"
                );
                false
            }
        };
        Ok(self.reclaim(None).await? || erased)
    }

    async fn reclaim_erased(&self) -> Result<bool> {
        let entries: Vec<(String, String)> = sqlx::query_as(
            "SELECT file_id,attempt_id FROM erased_download_files WHERE host_device=? ORDER BY file_id LIMIT 256",
        )
        .bind(self.host.as_str())
        .fetch_all(&self.store.pool)
        .await?;
        let (mut removed, mut retained) = (0_u64, 0_u64);
        for (file, attempt) in entries {
            let outcome = match (stored_uuid(&file), stored_uuid(&attempt)) {
                (Ok(file), Ok(attempt)) => self.files.remove_owned(file, attempt).await,
                (Err(error), _) | (_, Err(error)) => Err(error),
            };
            if outcome.is_err() {
                retained += 1;
                continue;
            }
            sqlx::query("DELETE FROM erased_download_files WHERE file_id=?")
                .bind(&file)
                .execute(&self.store.pool)
                .await?;
            removed += 1;
        }
        if retained > 0 {
            tracing::warn!(
                event = "erased_download_files_retained",
                retained,
                removed,
                "Erased account download files could not be removed and stay queued"
            );
        }
        Ok(removed > 0)
    }

    pub(super) async fn reclaim_file(&self, file: &str) -> Result<bool> {
        self.reclaim(Some(file)).await
    }

    async fn reclaim(&self, selection: Option<&str>) -> Result<bool> {
        let started = Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut query =
            QueryBuilder::new("SELECT id,attempt_id FROM download_files WHERE host_device=");
        query
            .push_bind(self.host.as_str())
            .push(" AND state='deleting'");
        if let Some(file) = selection {
            query.push(" AND id=").push_bind(file);
        } else {
            query.push(" AND cleanup_after<=unixepoch()");
        }
        query.push(" ORDER BY cleanup_after,created_at,id LIMIT 1");
        let file: Option<(String, String)> =
            query.build_query_as().fetch_optional(&mut *tx).await?;
        let Some((file, attempt)) = file else {
            tx.commit().await?;
            return Ok(false);
        };
        sqlx::query(
            "UPDATE download_files SET cleanup_after=unixepoch()+2 WHERE id=? AND state='deleting'",
        )
        .bind(&file)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.files
            .remove_owned(stored_uuid(&file)?, stored_uuid(&attempt)?)
            .await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let source: Option<String> = sqlx::query_scalar(
            "SELECT source_id FROM download_files WHERE id=? AND state='deleting'",
        )
        .bind(&file)
        .fetch_optional(&mut *tx)
        .await?
        .flatten();
        sqlx::query("UPDATE download_files SET state='deleted',source_id=NULL WHERE id=? AND state='deleting'")
            .bind(&file).execute(&mut *tx).await?;
        if let Some(source) = source {
            sqlx::query("DELETE FROM sources WHERE id=? AND provider='download' AND provider_id=?")
                .bind(source)
                .bind(&file)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        tracing::debug!(file_id = %file, outcome = "reclaimed", elapsed_ms = started.elapsed().as_millis() as u64, "Owned download file reclaimed");
        Ok(true)
    }
}

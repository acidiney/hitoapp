use super::WatchCounters;
use crate::{LibraryStorage, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ScanRequest {
    Queued(String),
    Covered(String),
    Busy(String),
    Superseded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JobEnd {
    Complete,
    Failed,
}

impl LibraryStorage {
    pub(super) async fn request_watched_scan(
        &self,
        folder: &str,
        generation: i64,
        counters: &WatchCounters,
    ) -> Result<ScanRequest> {
        if let Some(request) = self.active_scan(folder, generation).await? {
            return Ok(request);
        }
        WatchCounters::add(&counters.write_requests);
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let library: Option<String> = sqlx::query_scalar(
            "SELECT library_id FROM music_folders
             WHERE id = ? AND host_device = ? AND generation = ? AND connected = 1 AND bound = 1",
        )
        .bind(folder)
        .bind(self.host.as_str())
        .bind(generation)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(library) = library else {
            return Ok(ScanRequest::Superseded);
        };
        let active: Option<(String, String, bool)> = sqlx::query_as(
            "SELECT id, state, lease_until IS NULL OR lease_until < unixepoch() FROM jobs
             WHERE folder_id = ? AND folder_generation = ? AND state IN ('queued','running') LIMIT 1",
        )
        .bind(folder)
        .bind(generation)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((id, state, expired)) = active {
            return Ok(if state == "queued" || expired {
                ScanRequest::Covered(id)
            } else {
                ScanRequest::Busy(id)
            });
        }
        let owner: String = sqlx::query_scalar(
            "SELECT m.user_id FROM library_members m JOIN users u ON u.id = m.user_id
             WHERE m.library_id = ? AND u.admin = 1 ORDER BY m.user_id LIMIT 1",
        )
        .bind(&library)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(crate::Error::NotFound)?;
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO jobs(id, user_id, library_id, folder_id, folder_generation, kind)
             VALUES (?, ?, ?, ?, ?, 'scan')",
        )
        .bind(&id)
        .bind(owner)
        .bind(library)
        .bind(folder)
        .bind(generation)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ScanRequest::Queued(id))
    }

    async fn active_scan(&self, folder: &str, generation: i64) -> Result<Option<ScanRequest>> {
        let active: Option<(String, String, bool)> = sqlx::query_as(
            "SELECT j.id, j.state, j.lease_until IS NULL OR j.lease_until < unixepoch()
             FROM jobs j JOIN music_folders f ON f.id = j.folder_id
             WHERE j.folder_id = ? AND j.folder_generation = ? AND j.state IN ('queued','running')
               AND f.generation = ? AND f.connected = 1 AND f.bound = 1 LIMIT 1",
        )
        .bind(folder)
        .bind(generation)
        .bind(generation)
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(active.map(|(id, state, expired)| {
            if state == "queued" || expired {
                ScanRequest::Covered(id)
            } else {
                ScanRequest::Busy(id)
            }
        }))
    }

    pub(super) async fn job_end(&self, job: &str) -> Result<Option<JobEnd>> {
        let state: Option<String> = sqlx::query_scalar("SELECT state FROM jobs WHERE id = ?")
            .bind(job)
            .fetch_optional(&self.store.pool)
            .await?;
        Ok(match state.as_deref() {
            Some("queued" | "running") => None,
            Some("failed") => Some(JobEnd::Failed),
            _ => Some(JobEnd::Complete),
        })
    }
}

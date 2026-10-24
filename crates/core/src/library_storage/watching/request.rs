use crate::{LibraryStorage, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScanRequest {
    Queued,
    Covered,
    Busy,
    Superseded,
}

impl LibraryStorage {
    pub(super) async fn request_watched_scan(
        &self,
        folder: &str,
        generation: i64,
    ) -> Result<ScanRequest> {
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
        let active: Option<(String, bool)> = sqlx::query_as(
            "SELECT state, lease_until IS NULL OR lease_until < unixepoch() FROM jobs
             WHERE folder_id = ? AND folder_generation = ? AND state IN ('queued','running') LIMIT 1",
        )
        .bind(folder)
        .bind(generation)
        .fetch_optional(&mut *tx)
        .await?;
        match active {
            Some((state, expired)) if state == "queued" || expired => {
                return Ok(ScanRequest::Covered);
            }
            Some(_) => return Ok(ScanRequest::Busy),
            None => {}
        }
        let owner: String = sqlx::query_scalar(
            "SELECT m.user_id FROM library_members m JOIN users u ON u.id = m.user_id
             WHERE m.library_id = ? AND u.admin = 1 ORDER BY m.user_id LIMIT 1",
        )
        .bind(&library)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(crate::Error::NotFound)?;
        sqlx::query(
            "INSERT INTO jobs(id, user_id, library_id, folder_id, folder_generation, kind)
             VALUES (?, ?, ?, ?, ?, 'scan')",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(owner)
        .bind(library)
        .bind(folder)
        .bind(generation)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ScanRequest::Queued)
    }
}

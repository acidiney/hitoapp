use super::folder_types::Folder;
use crate::{Error, Result, store::Store};
use sqlx::{Sqlite, Transaction};

pub(super) struct Lease {
    pub job: String,
    pub token: String,
    pub folder: Folder,
}

impl Lease {
    pub async fn check_folder(&self, store: &Store, folder: &Folder) -> Result<()> {
        let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_folders WHERE id = ? AND generation = ? AND connected = 1 AND bound = 1)")
            .bind(&folder.id).bind(folder.generation).fetch_one(&store.pool).await?;
        if active { Ok(()) } else { Err(stopped()) }
    }

    pub async fn guard(&self, tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM jobs j JOIN music_folders f ON f.id = j.folder_id
             WHERE j.id = ? AND j.lease_token = ? AND j.state = 'running' AND j.lease_until > unixepoch()
               AND f.id = ? AND f.generation = ? AND j.folder_generation = f.generation AND f.connected = 1 AND f.bound = 1)",
        ).bind(&self.job).bind(&self.token).bind(&self.folder.id).bind(self.folder.generation)
            .fetch_one(&mut **tx).await?;
        if valid { Ok(()) } else { Err(stopped()) }
    }

    pub async fn renew(&self, store: &Store, count: i64) -> Result<()> {
        let mut tx = store.pool.begin_with("BEGIN IMMEDIATE").await?;
        self.guard(&mut tx).await?;
        sqlx::query("UPDATE jobs SET lease_until = unixepoch() + 300, progress = ? WHERE id = ? AND lease_token = ?")
            .bind(count).bind(&self.job).bind(&self.token).execute(&mut *tx).await?;
        tx.commit().await?;
        tracing::debug!(job_id = %self.job, files_read = count, "Scan lease renewed");
        Ok(())
    }
}

pub(super) fn stopped() -> Error {
    Error::Conflict("Scan was cancelled or superseded".into())
}

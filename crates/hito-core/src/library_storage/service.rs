use super::{Job, LibraryStorage, folder_types::Folder};
use crate::{Actor, Error, Result, identity::library_access};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

impl LibraryStorage {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), library_id, host_device = self.host.as_str()))]
    pub async fn enqueue_scan(&self, actor: &Actor, library: &str) -> Result<Vec<Job>> {
        actor.require_admin()?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        library_access(&mut tx, actor, library).await?;
        crate::identity::shared_library(&mut tx, library).await?;
        tracing::Span::current().record("library_id", library);
        self.ensure_legacy(&mut tx, actor, library).await?;
        let folders: Vec<Folder> = sqlx::query_as("SELECT * FROM music_folders WHERE library_id = ? AND host_device = ? AND connected = 1 AND bound = 1 ORDER BY id")
            .bind(library).bind(self.host.as_str()).fetch_all(&mut *tx).await?;
        let mut ids = Vec::new();
        for folder in folders {
            ids.push(queue(&mut tx, actor, &folder).await?);
        }
        tx.commit().await?;
        let mut jobs = Vec::new();
        for id in ids {
            jobs.push(self.job(actor, &id).await?);
        }
        tracing::info!(jobs = jobs.len(), "Folder scans queued or selected");
        Ok(jobs)
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), folder_id))]
    pub async fn enqueue_folder_scan(&self, actor: &Actor, folder: &str) -> Result<Job> {
        actor.require_admin()?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let folder: Folder =
            sqlx::query_as("SELECT * FROM music_folders WHERE id = ? AND host_device = ?")
                .bind(folder)
                .bind(self.host.as_str())
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(Error::NotFound)?;
        library_access(&mut tx, actor, &folder.library_id).await?;
        tracing::Span::current().record("folder_id", &folder.id);
        if !folder.connected || !folder.bound {
            return Err(Error::Conflict(
                "Reconnect this music folder before scanning".into(),
            ));
        }
        let id = queue(&mut tx, actor, &folder).await?;
        tx.commit().await?;
        self.job(actor, &id).await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), job_id))]
    pub async fn job(&self, actor: &Actor, id: &str) -> Result<Job> {
        actor.require_admin()?;
        let job: Job = sqlx::query_as(
            "SELECT j.id, j.library_id, j.folder_id, j.kind, j.state, j.progress, j.error FROM jobs j
             JOIN library_members m ON m.library_id = j.library_id
             WHERE j.id = ? AND m.user_id = ?",
        ).bind(id).bind(actor.id()).fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        tracing::Span::current().record("job_id", &job.id);
        Ok(job)
    }
}

pub(super) async fn queue(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    folder: &Folder,
) -> Result<String> {
    sqlx::query("INSERT OR IGNORE INTO jobs(id, user_id, library_id, folder_id, folder_generation, kind) VALUES (?, ?, ?, ?, ?, 'scan')")
        .bind(Uuid::new_v4().to_string()).bind(actor.id()).bind(&folder.library_id).bind(&folder.id).bind(folder.generation).execute(&mut **tx).await?;
    Ok(sqlx::query_scalar("SELECT id FROM jobs WHERE folder_id = ? AND folder_generation = ? AND state IN ('queued','running') LIMIT 1")
        .bind(&folder.id).bind(folder.generation).fetch_one(&mut **tx).await?)
}

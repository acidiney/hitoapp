use super::{Indexer, Job};
use crate::{Actor, Error, Result, identity::library_access};
use uuid::Uuid;

impl Indexer {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn enqueue_scan(&self, actor: &Actor, library: &str) -> Result<Job> {
        actor.require_admin()?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let id = match select_scan(&mut tx, actor, library).await {
            Ok(id) => {
                tx.commit().await?;
                id
            }
            Err(error) => {
                tx.rollback().await?;
                tracing::warn!(error_code = error.code(), "Scan enqueue rolled back");
                return Err(error);
            }
        };
        tracing::info!(job_id = %id, "Scan queued or existing scan selected");
        self.job(actor, &id).await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn job(&self, actor: &Actor, id: &str) -> Result<Job> {
        sqlx::query_as(
            "SELECT id, kind, state, progress, error FROM jobs WHERE id = ? AND user_id = ?",
        )
        .bind(id)
        .bind(actor.id())
        .fetch_optional(&self.store.pool)
        .await?
        .ok_or(Error::NotFound)
    }
}

async fn select_scan(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    library: &str,
) -> Result<String> {
    library_access(tx, actor, library).await?;
    sqlx::query(
        "INSERT OR IGNORE INTO jobs(id, user_id, library_id, kind) VALUES (?, ?, ?, 'scan')",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(actor.id())
    .bind(library)
    .execute(&mut **tx)
    .await?;
    Ok(sqlx::query_scalar(
        "SELECT id FROM jobs
         WHERE library_id = ? AND kind = 'scan' AND state IN ('queued', 'running')
         ORDER BY created_at LIMIT 1",
    )
    .bind(library)
    .fetch_one(&mut **tx)
    .await?)
}

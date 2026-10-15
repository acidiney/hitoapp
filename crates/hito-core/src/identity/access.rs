use super::{Actor, Identity};
use crate::{Error, Result};
use sqlx::SqliteConnection;

impl Identity {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), allowed))]
    pub async fn set_library_access(
        &self,
        actor: &Actor,
        library: &str,
        user: &str,
        allowed: bool,
    ) -> Result<()> {
        actor.require_admin()?;
        if allowed {
            let exists: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM libraries l CROSS JOIN users u WHERE l.id = ? AND u.id = ?",
            )
            .bind(library)
            .bind(user)
            .fetch_one(&self.store.pool)
            .await?;
            if exists == 0 {
                return Err(Error::NotFound);
            }
            sqlx::query("INSERT OR IGNORE INTO library_members VALUES (?, ?)")
                .bind(library)
                .bind(user)
                .execute(&self.store.pool)
                .await?;
        } else {
            sqlx::query("DELETE FROM library_members WHERE library_id = ? AND user_id = ?")
                .bind(library)
                .bind(user)
                .execute(&self.store.pool)
                .await?;
        }
        tracing::info!(allowed, "Library membership updated");
        Ok(())
    }
}

#[tracing::instrument(level = "trace", skip_all, err(Debug), fields(account_id = actor.id()))]
pub(crate) async fn library_access(
    connection: &mut SqliteConnection,
    actor: &Actor,
    library: &str,
) -> Result<()> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM library_members WHERE user_id = ? AND library_id = ?",
    )
    .bind(actor.id())
    .bind(library)
    .fetch_one(connection)
    .await?;
    if count == 1 {
        Ok(())
    } else {
        Err(Error::NotFound)
    }
}

#[tracing::instrument(level = "trace", skip_all, err(Debug), fields(account_id = actor.id()))]
pub(crate) async fn recording_access(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<()> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM recordings r
         JOIN library_members m ON m.library_id = r.library_id
         WHERE r.id = ? AND m.user_id = ?",
    )
    .bind(recording)
    .bind(actor.id())
    .fetch_one(connection)
    .await?;
    if count == 1 {
        Ok(())
    } else {
        Err(Error::NotFound)
    }
}

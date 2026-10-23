use super::{Actor, Identity};
use crate::{Error, Result};
use sqlx::{Sqlite, SqliteConnection, Transaction};
use uuid::Uuid;

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
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        shared_library(&mut tx, library).await?;
        if allowed {
            let exists: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM libraries l CROSS JOIN users u WHERE l.id = ? AND u.id = ?",
            )
            .bind(library)
            .bind(user)
            .fetch_one(&mut *tx)
            .await?;
            if exists == 0 {
                return Err(Error::NotFound);
            }
            let inserted = sqlx::query("INSERT OR IGNORE INTO library_members VALUES (?, ?)")
                .bind(library)
                .bind(user)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if inserted == 1 {
                crate::metadata::provision(&mut tx, user, library).await?;
            }
        } else {
            sqlx::query("DELETE FROM library_members WHERE library_id = ? AND user_id = ?")
                .bind(library)
                .bind(user)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        tracing::info!(allowed, "Library membership updated");
        Ok(())
    }
}

pub(crate) async fn personal_library(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
) -> Result<String> {
    sqlx::query("INSERT INTO libraries(id,name,owner_id) VALUES (?,'Personal providers',?) ON CONFLICT(owner_id) WHERE owner_id IS NOT NULL DO NOTHING")
        .bind(Uuid::new_v4().to_string()).bind(owner).execute(&mut **tx).await?;
    let id: String = sqlx::query_scalar("SELECT id FROM libraries WHERE owner_id = ?")
        .bind(owner)
        .fetch_one(&mut **tx)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO library_members(library_id,user_id) VALUES (?,?)")
        .bind(&id)
        .bind(owner)
        .execute(&mut **tx)
        .await?;
    Ok(id)
}

pub(crate) async fn shared_library(connection: &mut SqliteConnection, library: &str) -> Result<()> {
    let owner: Option<Option<String>> =
        sqlx::query_scalar("SELECT owner_id FROM libraries WHERE id = ?")
            .bind(library)
            .fetch_optional(connection)
            .await?;
    match owner {
        Some(None) => Ok(()),
        Some(Some(_)) => Err(Error::Invalid(
            "Personal provider libraries are managed by their owner through plugins".into(),
        )),
        None => Err(Error::NotFound),
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

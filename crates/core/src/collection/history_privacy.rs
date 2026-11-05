use super::{Collection, HistoryCleared, HistoryPreference};
use crate::{Actor, Result};
use std::time::Instant;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn history_preference(&self, actor: &Actor) -> Result<HistoryPreference> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let enabled = read_enabled(&mut tx, actor).await?;
        tx.commit().await?;
        Ok(HistoryPreference { enabled })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), history_enabled = enabled))]
    pub async fn set_history_enabled(
        &self,
        actor: &Actor,
        enabled: bool,
    ) -> Result<HistoryPreference> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        sqlx::query(
            "INSERT INTO account_history_preferences(user_id, history_enabled) VALUES (?, ?)
             ON CONFLICT(user_id) DO UPDATE SET history_enabled = excluded.history_enabled",
        )
        .bind(actor.id())
        .bind(enabled)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::info!(outcome = "success", "Listening history preference changed");
        Ok(HistoryPreference { enabled })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn clear_history(&self, actor: &Actor) -> Result<HistoryCleared> {
        let started = Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let removed = sqlx::query("DELETE FROM listening_history WHERE user_id = ?")
            .bind(actor.id())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        tracing::info!(
            outcome = "success",
            removed,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Listening history deleted"
        );
        Ok(HistoryCleared { removed })
    }
}

async fn read_enabled(connection: &mut sqlx::SqliteConnection, actor: &Actor) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT history_enabled FROM account_history_preferences WHERE user_id = ?",
    )
    .bind(actor.id())
    .fetch_optional(connection)
    .await?
    .unwrap_or(true))
}

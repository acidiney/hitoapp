use super::{Downloads, lease::stored_uuid};
use crate::{Actor, Error, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

pub(super) const DEFAULT_BUDGET_BYTES: i64 = 5 * 1024 * 1024 * 1024;
const MAX_BUDGET_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct AutomaticCopies {
    pub keep_played_tracks: bool,
    #[schema(minimum = 0)]
    pub used_bytes: u64,
    #[schema(minimum = 0)]
    pub budget_bytes: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AutomaticCopyPreference {
    pub keep_played_tracks: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AutomaticCopyBudget {
    #[schema(minimum = 0, maximum = 1099511627776_u64)]
    pub budget_bytes: u64,
}

pub(super) struct Retired(pub Vec<(String, String)>);

impl Downloads {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn automatic_copies(&self, actor: &Actor) -> Result<AutomaticCopies> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let (keep_played_tracks, budget_bytes) = preference_on(&mut tx, actor.id()).await?;
        let used_bytes = self.used_bytes_on(&mut tx, actor.id()).await?;
        tx.commit().await?;
        Ok(AutomaticCopies {
            keep_played_tracks,
            used_bytes,
            budget_bytes: budget_bytes as u64,
        })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), keep_played_tracks = preference.keep_played_tracks))]
    pub async fn set_automatic_copies(
        &self,
        actor: &Actor,
        preference: AutomaticCopyPreference,
    ) -> Result<AutomaticCopies> {
        actor.require_unlocked()?;
        actor.require_session()?;
        let started = std::time::Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        sqlx::query(
            "INSERT INTO account_automatic_copies(user_id,keep_played_tracks) VALUES(?1,?2)
             ON CONFLICT(user_id) DO UPDATE SET keep_played_tracks=excluded.keep_played_tracks",
        )
        .bind(actor.id())
        .bind(preference.keep_played_tracks)
        .execute(&mut *tx)
        .await?;
        let retired = if preference.keep_played_tracks {
            Retired(Vec::new())
        } else {
            self.retire_all_copies_on(&mut tx, actor.id()).await?
        };
        tx.commit().await?;
        let removed = retired.0.len();
        self.reclaim_retired(retired).await;
        tracing::info!(
            event = "automatic_copies_preference_changed",
            removed,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Keep played tracks setting saved"
        );
        self.automatic_copies(actor).await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn automatic_copy_budget(
        &self,
        actor: &Actor,
        account: &str,
    ) -> Result<AutomaticCopyBudget> {
        actor.require_admin()?;
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        existing_account_on(&mut tx, account).await?;
        let (_, budget) = preference_on(&mut tx, account).await?;
        tx.commit().await?;
        Ok(AutomaticCopyBudget {
            budget_bytes: budget as u64,
        })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), budget_bytes = budget.budget_bytes))]
    pub async fn set_automatic_copy_budget(
        &self,
        actor: &Actor,
        account: &str,
        budget: AutomaticCopyBudget,
    ) -> Result<AutomaticCopyBudget> {
        actor.require_admin()?;
        actor.require_unlocked()?;
        if budget.budget_bytes > MAX_BUDGET_BYTES {
            return Err(Error::Invalid(
                "Use a storage budget between 0 bytes and 1 TiB".into(),
            ));
        }
        let started = std::time::Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        existing_account_on(&mut tx, account).await?;
        sqlx::query(
            "INSERT INTO account_automatic_copies(user_id,budget_bytes) VALUES(?1,?2)
             ON CONFLICT(user_id) DO UPDATE SET budget_bytes=excluded.budget_bytes",
        )
        .bind(account)
        .bind(budget.budget_bytes as i64)
        .execute(&mut *tx)
        .await?;
        let retired = self
            .evict_on(&mut tx, account, super::eviction::Room::WithinBudget)
            .await?;
        tx.commit().await?;
        let evicted = retired.0.len();
        self.reclaim_retired(retired).await;
        tracing::info!(
            event = "automatic_copy_budget_changed",
            evicted,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Automatic copy budget saved"
        );
        Ok(budget)
    }

    pub(super) async fn used_bytes_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
    ) -> Result<u64> {
        let used: i64 = sqlx::query_scalar(
            "SELECT coalesce(sum(file.byte_length),0) FROM automatic_copies copy
             JOIN download_intents intent ON intent.account_id=copy.account_id AND intent.recording_id=copy.recording_id
                AND intent.host_device=copy.host_device AND intent.access_grant_id=copy.access_grant_id AND intent.state='ready'
             JOIN download_files file ON file.id=intent.file_id AND file.state='ready'
             WHERE copy.account_id=? AND copy.host_device=?
                AND NOT EXISTS(SELECT 1 FROM chosen_download_intents chosen WHERE chosen.id=intent.id)",
        )
        .bind(account)
        .bind(self.host.as_str())
        .fetch_one(connection)
        .await?;
        Ok(used.max(0) as u64)
    }

    async fn retire_all_copies_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
    ) -> Result<Retired> {
        let recordings: Vec<String> = sqlx::query_scalar(
            "DELETE FROM automatic_copies WHERE account_id=? AND host_device=? RETURNING recording_id",
        )
        .bind(account)
        .bind(self.host.as_str())
        .fetch_all(&mut *connection)
        .await?;
        let mut retired = Vec::new();
        for recording in recordings {
            if let Some(file) = self.retire_copy_on(connection, account, &recording).await? {
                retired.push(file);
            }
        }
        Ok(Retired(retired))
    }

    pub(super) async fn retire_copy_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        recording: &str,
    ) -> Result<Option<(String, String)>> {
        let Some((file, attempt)) = self
            .retire_unreferenced_on(connection, account, recording)
            .await?
        else {
            return Ok(None);
        };
        let source: Option<String> =
            sqlx::query_scalar("SELECT source_id FROM download_files WHERE id=?")
                .bind(&file)
                .fetch_optional(&mut *connection)
                .await?
                .flatten();
        if let Some(source) = source {
            sqlx::query("UPDATE download_files SET source_id=NULL WHERE id=?")
                .bind(&file)
                .execute(&mut *connection)
                .await?;
            sqlx::query("DELETE FROM sources WHERE id=? AND provider='download' AND provider_id=?")
                .bind(source)
                .bind(&file)
                .execute(&mut *connection)
                .await?;
        }
        Ok(Some((file, attempt)))
    }

    pub(super) async fn reclaim_retired(&self, retired: Retired) {
        for (file, attempt) in retired.0 {
            let revoked = stored_uuid(&file)
                .and_then(|file| Ok((file, stored_uuid(&attempt)?)))
                .and_then(|(file, attempt)| self.files.revoke(file, attempt));
            let reclaimed = match revoked {
                Ok(()) => self.reclaim_file(&file).await.map(|_| ()),
                Err(error) => Err(error),
            };
            if let Err(error) = reclaimed {
                tracing::debug!(
                    error_code = error.code(),
                    outcome = "cleanup_pending",
                    "Automatic copy file cleanup deferred"
                );
            }
        }
    }
}

pub(super) async fn preference_on(
    connection: &mut SqliteConnection,
    account: &str,
) -> Result<(bool, i64)> {
    let stored: Option<(bool, i64)> = sqlx::query_as(
        "SELECT keep_played_tracks,budget_bytes FROM account_automatic_copies WHERE user_id=?",
    )
    .bind(account)
    .fetch_optional(connection)
    .await?;
    Ok(stored.unwrap_or((true, DEFAULT_BUDGET_BYTES)))
}

async fn existing_account_on(connection: &mut SqliteConnection, account: &str) -> Result<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=?)")
        .bind(account)
        .fetch_one(connection)
        .await?;
    if exists { Ok(()) } else { Err(Error::NotFound) }
}

use super::{Actor, Identity, LockStatus, Unlock, credentials};
use crate::{Error, Result};

const MAX_WRONG_PINS: i64 = 5;

const STATUS: &str = "SELECT p.idle_minutes, COALESCE(l.locked, 0)
    FROM account_pin_locks p LEFT JOIN session_locks l ON l.token_hash = ?
    WHERE p.user_id = ?";

impl Identity {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn lock_status(&self, actor: &Actor) -> Result<LockStatus> {
        self.status(actor.id(), &actor.session_hash).await
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn lock(&self, actor: &Actor) -> Result<LockStatus> {
        actor
            .check_session_exists_on(&mut *self.store.pool.acquire().await?)
            .await?;
        let enabled: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM account_pin_locks WHERE user_id = ?)")
                .bind(actor.id())
                .fetch_one(&self.store.pool)
                .await?;
        if !enabled {
            return Err(Error::Invalid("Set a PIN before locking a session".into()));
        }
        self.mark_locked(&actor.session_hash).await?;
        self.changes.sessions_changed(actor.id());
        tracing::info!(
            event = "session_locked",
            reason = "manual",
            "Session locked"
        );
        self.status(actor.id(), &actor.session_hash).await
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn unlock(&self, actor: &Actor, request: Unlock) -> Result<LockStatus> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_exists_on(&mut tx).await?;
        let row: Option<(String, bool, i64)> = sqlx::query_as(
            "SELECT p.pin_hash, COALESCE(l.locked, 0), COALESCE(l.failures, 0)
             FROM account_pin_locks p LEFT JOIN session_locks l ON l.token_hash = ?
             WHERE p.user_id = ?",
        )
        .bind(&actor.session_hash)
        .bind(actor.id())
        .fetch_optional(&mut *tx)
        .await?;
        let Some((hash, true, failures)) = row else {
            tx.commit().await?;
            return self.status(actor.id(), &actor.session_hash).await;
        };
        if failures >= MAX_WRONG_PINS {
            self.end_session(&mut tx, actor).await?;
            tx.commit().await?;
            self.changes.sessions_changed(actor.id());
            return Err(Error::Unauthorized);
        }
        sqlx::query("UPDATE session_locks SET failures = failures + 1 WHERE token_hash = ?")
            .bind(&actor.session_hash)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        if is_pin(&request.pin) && credentials::verify(request.pin, hash).await? {
            sqlx::query(
                "UPDATE session_locks SET locked = 0, failures = 0, active_at = unixepoch()
                 WHERE token_hash = ?",
            )
            .bind(&actor.session_hash)
            .execute(&self.store.pool)
            .await?;
            tracing::info!(event = "session_unlocked", "Session unlocked");
            return self.status(actor.id(), &actor.session_hash).await;
        }
        let wrong = failures + 1;
        if wrong >= MAX_WRONG_PINS {
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            self.end_session(&mut tx, actor).await?;
            tx.commit().await?;
            self.changes.sessions_changed(actor.id());
            return Err(Error::Unauthorized);
        }
        tracing::info!(
            event = "unlock_rejected",
            attempts_left = MAX_WRONG_PINS - wrong,
            "Unlock rejected"
        );
        Err(Error::Invalid(format!(
            "That PIN is not correct. {} attempts left",
            MAX_WRONG_PINS - wrong
        )))
    }

    pub(super) async fn session_locked(&self, account: &str, session_hash: &str) -> Result<bool> {
        let row: Option<(i64, Option<bool>, i64)> = sqlx::query_as(
            "SELECT p.idle_minutes, l.locked, unixepoch() - COALESCE(l.active_at, unixepoch())
             FROM account_pin_locks p LEFT JOIN session_locks l ON l.token_hash = ?
             WHERE p.user_id = ?",
        )
        .bind(session_hash)
        .bind(account)
        .fetch_optional(&self.store.pool)
        .await?;
        let Some((idle_minutes, locked, quiet_seconds)) = row else {
            return Ok(false);
        };
        if locked == Some(true) {
            return Ok(true);
        }
        if idle_minutes > 0 && quiet_seconds > idle_minutes * 60 {
            self.mark_locked(session_hash).await?;
            self.changes.sessions_changed(account);
            tracing::info!(event = "session_locked", reason = "idle", "Session locked");
            return Ok(true);
        }
        if locked.is_none() {
            sqlx::query("INSERT OR IGNORE INTO session_locks(token_hash) VALUES (?)")
                .bind(session_hash)
                .execute(&self.store.pool)
                .await?;
        }
        Ok(false)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn record_activity(&self, actor: &Actor) -> Result<LockStatus> {
        if !actor.locked {
            sqlx::query(
                "UPDATE session_locks SET active_at = unixepoch() WHERE token_hash = ? AND locked = 0",
            )
            .bind(&actor.session_hash)
            .execute(&self.store.pool)
            .await?;
        }
        self.status(actor.id(), &actor.session_hash).await
    }

    async fn mark_locked(&self, session_hash: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO session_locks(token_hash, locked) VALUES (?, 1)
             ON CONFLICT(token_hash) DO UPDATE SET locked = 1",
        )
        .bind(session_hash)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }

    async fn end_session(&self, tx: &mut sqlx::SqliteConnection, actor: &Actor) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(&actor.session_hash)
            .execute(tx)
            .await?;
        tracing::warn!(
            event = "session_ended_after_wrong_pins",
            wrong_pins = MAX_WRONG_PINS,
            "Session ended after repeated wrong PINs"
        );
        Ok(())
    }

    pub(super) async fn status(&self, account: &str, session_hash: &str) -> Result<LockStatus> {
        let row: Option<(i64, bool)> = sqlx::query_as(STATUS)
            .bind(session_hash)
            .bind(account)
            .fetch_optional(&self.store.pool)
            .await?;
        Ok(match row {
            Some((idle_minutes, locked)) => LockStatus {
                pin_enabled: true,
                idle_minutes: idle_minutes as u32,
                locked,
            },
            None => LockStatus {
                pin_enabled: false,
                idle_minutes: 0,
                locked: false,
            },
        })
    }
}

pub(super) fn is_pin(value: &str) -> bool {
    (4..=8).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

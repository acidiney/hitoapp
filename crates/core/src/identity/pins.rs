use super::{Actor, Identity, LockStatus, RemovePin, SetPin, credentials, locks::is_pin};
use crate::{Error, Result};

const IDLE_CHOICES: [u32; 4] = [0, 5, 15, 60];

impl Identity {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn set_pin(&self, actor: &Actor, request: SetPin) -> Result<LockStatus> {
        actor.require_unlocked()?;
        if !IDLE_CHOICES.contains(&request.idle_minutes) {
            return Err(Error::Invalid(
                "Choose to lock never, or after 5, 15 or 60 minutes".into(),
            ));
        }
        if let Some(pin) = &request.pin {
            validate_pin(pin)?;
        }
        self.confirm_password(actor, request.password).await?;
        let new_hash = match &request.pin {
            Some(pin) => Some(credentials::hash(pin.clone()).await?),
            None => None,
        };
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let changed_pin = new_hash.is_some();
        match new_hash {
            Some(hash) => {
                sqlx::query(
                    "INSERT INTO account_pin_locks(user_id, pin_hash, idle_minutes) VALUES (?, ?, ?)
                     ON CONFLICT(user_id) DO UPDATE SET pin_hash = excluded.pin_hash,
                         idle_minutes = excluded.idle_minutes, updated_at = unixepoch()",
                )
                .bind(actor.id())
                .bind(hash)
                .bind(request.idle_minutes)
                .execute(&mut *tx)
                .await?;
            }
            None => {
                let updated = sqlx::query(
                    "UPDATE account_pin_locks SET idle_minutes = ?, updated_at = unixepoch()
                     WHERE user_id = ?",
                )
                .bind(request.idle_minutes)
                .bind(actor.id())
                .execute(&mut *tx)
                .await?
                .rows_affected();
                if updated == 0 {
                    return Err(Error::Invalid("Choose a PIN first".into()));
                }
            }
        }
        sqlx::query(
            "UPDATE session_locks SET failures = 0 WHERE token_hash IN
             (SELECT token_hash FROM sessions WHERE user_id = ?)",
        )
        .bind(actor.id())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO session_locks(token_hash) VALUES (?)
             ON CONFLICT(token_hash) DO UPDATE SET active_at = unixepoch() WHERE locked = 0",
        )
        .bind(&actor.session_hash)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::info!(
            event = "pin_set",
            changed_pin,
            idle_minutes = request.idle_minutes,
            "Account PIN saved"
        );
        self.status(actor.id(), &actor.session_hash).await
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn remove_pin(&self, actor: &Actor, request: RemovePin) -> Result<LockStatus> {
        actor.require_unlocked()?;
        self.confirm_password(actor, request.password).await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM account_pin_locks WHERE user_id = ?")
            .bind(actor.id())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM session_locks WHERE token_hash IN
             (SELECT token_hash FROM sessions WHERE user_id = ?)",
        )
        .bind(actor.id())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::info!(event = "pin_removed", "Account PIN removed");
        self.status(actor.id(), &actor.session_hash).await
    }

    async fn confirm_password(&self, actor: &Actor, password: String) -> Result<()> {
        let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
            .bind(actor.id())
            .fetch_optional(&self.store.pool)
            .await?
            .ok_or(Error::Unauthorized)?;
        if password.len() <= 1024 && credentials::verify(password, hash).await? {
            Ok(())
        } else {
            Err(Error::Invalid("That password is not correct".into()))
        }
    }
}

fn validate_pin(pin: &str) -> Result<()> {
    if is_pin(pin) {
        Ok(())
    } else {
        Err(Error::Invalid("Use 4 to 8 digits for the PIN".into()))
    }
}

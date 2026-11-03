use super::{Actor, ChangePassword, Identity, SessionsRevoked, credentials};
use crate::{Error, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MAX_WRONG_PASSWORDS: u32 = 5;
const WINDOW: Duration = Duration::from_secs(900);

#[derive(Clone, Default)]
pub(crate) struct PasswordAttempts {
    accounts: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
}

impl PasswordAttempts {
    fn reserve(&self, account: &str) -> Result<()> {
        let now = Instant::now();
        let mut accounts = self
            .accounts
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        accounts.retain(|_, (_, since)| now.duration_since(*since) < WINDOW);
        let (count, since) = accounts.entry(account.to_owned()).or_insert((0, now));
        if *count >= MAX_WRONG_PASSWORDS {
            let remaining = WINDOW.saturating_sub(now.duration_since(*since));
            return Err(Error::RateLimited(remaining.as_secs().max(1)));
        }
        *count += 1;
        Ok(())
    }

    fn clear(&self, account: &str) {
        self.accounts
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(account);
    }
}

impl Identity {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn change_password(
        &self,
        actor: &Actor,
        request: ChangePassword,
    ) -> Result<SessionsRevoked> {
        actor.require_unlocked()?;
        actor.require_session()?;
        credentials::validate_password(&request.new_password)?;
        self.password_attempts.reserve(actor.id())?;
        let current_hash: String =
            sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
                .bind(actor.id())
                .fetch_optional(&self.store.pool)
                .await?
                .ok_or(Error::Unauthorized)?;
        let correct = request.current_password.len() <= 1024
            && credentials::verify(request.current_password, current_hash.clone()).await?;
        if !correct {
            tracing::warn!(
                event = "password_change_rejected",
                "Password change refused"
            );
            return Err(Error::Unauthorized);
        }
        let new_hash = credentials::hash(request.new_password).await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let updated =
            sqlx::query("UPDATE users SET password_hash = ? WHERE id = ? AND password_hash = ?")
                .bind(new_hash)
                .bind(actor.id())
                .bind(current_hash)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        if updated == 0 {
            return Err(Error::Unauthorized);
        }
        let revoked = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND token_hash != ?")
            .bind(actor.id())
            .bind(actor.session_binding())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        self.password_attempts.clear(actor.id());
        tracing::info!(
            event = "password_changed",
            sessions_revoked = revoked,
            "Password changed"
        );
        Ok(SessionsRevoked { revoked })
    }
}

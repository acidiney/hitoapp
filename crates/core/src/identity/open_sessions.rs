use super::{Actor, Identity, SessionInfo, SessionsRevoked};
use crate::{Error, Result};
use sha2::{Digest, Sha256};

const MAX_LISTED: i64 = 200;

impl Identity {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn sessions(&self, actor: &Actor) -> Result<Vec<SessionInfo>> {
        actor.require_unlocked()?;
        actor.require_session()?;
        actor.check_session(&self.store).await?;
        let rows: Vec<(String, i64, i64, Option<String>)> = sqlx::query_as(
            "SELECT token_hash, created_at, last_seen_at, client FROM sessions
             WHERE user_id = ? AND expires_at > unixepoch()
             ORDER BY token_hash = ? DESC, created_at DESC, token_hash LIMIT ?",
        )
        .bind(actor.id())
        .bind(actor.session_binding())
        .bind(MAX_LISTED)
        .fetch_all(&self.store.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(token_hash, created_at, last_seen_at, client)| SessionInfo {
                    id: session_id(&token_hash),
                    created_at,
                    last_seen_at,
                    client,
                    current: token_hash == actor.session_binding(),
                },
            )
            .collect())
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn revoke_session(&self, actor: &Actor, id: &str) -> Result<bool> {
        actor.require_unlocked()?;
        actor.require_session()?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let hashes: Vec<String> = sqlx::query_scalar(
            "SELECT token_hash FROM sessions WHERE user_id = ? AND expires_at > unixepoch()",
        )
        .bind(actor.id())
        .fetch_all(&mut *tx)
        .await?;
        let target = hashes
            .into_iter()
            .find(|token_hash| session_id(token_hash) == id)
            .ok_or(Error::NotFound)?;
        sqlx::query("DELETE FROM sessions WHERE token_hash = ? AND user_id = ?")
            .bind(&target)
            .bind(actor.id())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        let current = target == actor.session_binding();
        tracing::info!(
            event = "session_revoked",
            current,
            "Session revoked from the session list"
        );
        Ok(current)
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn revoke_other_sessions(&self, actor: &Actor) -> Result<SessionsRevoked> {
        actor.require_unlocked()?;
        actor.require_session()?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let revoked = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND token_hash != ?")
            .bind(actor.id())
            .bind(actor.session_binding())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        tracing::info!(
            event = "other_sessions_revoked",
            sessions_revoked = revoked,
            "Other sessions revoked"
        );
        Ok(SessionsRevoked { revoked })
    }
}

fn session_id(token_hash: &str) -> String {
    let digest = Sha256::digest(format!("hito.session.id.v1:{token_hash}").as_bytes());
    format!("{digest:x}")[..32].to_owned()
}

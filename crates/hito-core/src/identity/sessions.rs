use super::{Actor, AuthSession, Credentials, Identity, User, credentials};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use uuid::Uuid;

impl Identity {
    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn login(&self, credentials: Credentials) -> Result<AuthSession> {
        if credentials.password.len() > 1024 || credentials.username.len() > 64 {
            return Err(Error::Unauthorized);
        }
        let row: Option<(String, String, bool, String)> = sqlx::query_as(
            "SELECT id, username, admin, password_hash FROM users WHERE username = ?",
        )
        .bind(credentials.username.trim())
        .fetch_optional(&self.store.pool)
        .await?;
        let Some((id, username, admin, hash)) = row else {
            return Err(Error::Unauthorized);
        };
        if !credentials::verify(credentials.password, hash).await? {
            return Err(Error::Unauthorized);
        }
        self.create_session(User {
            id,
            username,
            admin,
        })
        .await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn create_session(&self, user: User) -> Result<AuthSession> {
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlx::query("INSERT INTO sessions VALUES (?, ?, unixepoch() + 2592000)")
            .bind(token_hash(&token))
            .bind(&user.id)
            .execute(&self.store.pool)
            .await?;
        tracing::info!(account_id = %user.id, "Session created");
        Ok(AuthSession { token, user })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn authenticate(&self, token: &str) -> Result<Actor> {
        let session_hash = token_hash(token);
        let user = sqlx::query_as(
            "SELECT u.id, u.username, u.admin
             FROM users u JOIN sessions s ON s.user_id = u.id
             WHERE s.token_hash = ? AND s.expires_at > unixepoch()",
        )
        .bind(&session_hash)
        .fetch_optional(&self.store.pool)
        .await?
        .ok_or(Error::Unauthorized)?;
        Ok(Actor { user, session_hash })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn logout(&self, token: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(token_hash(token))
            .execute(&self.store.pool)
            .await?;
        tracing::info!("Session revoked");
        Ok(())
    }
}

impl Actor {
    pub(crate) async fn check_session(&self, store: &crate::Store) -> Result<()> {
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE token_hash = ? AND user_id = ? AND expires_at > unixepoch())")
            .bind(&self.session_hash).bind(self.id()).fetch_one(&store.pool).await?;
        if valid {
            Ok(())
        } else {
            Err(Error::Unauthorized)
        }
    }
}

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

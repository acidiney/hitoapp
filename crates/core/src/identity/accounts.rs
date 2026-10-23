use super::{Actor, Credentials, Identity, User, credentials};
use crate::{Error, Result};
use uuid::Uuid;

impl Identity {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn create_user(&self, actor: &Actor, credentials: Credentials) -> Result<User> {
        actor.require_admin()?;
        let username = credentials::validate(&credentials)?;
        let hash = credentials::hash(credentials.password).await?;
        let user = User {
            id: Uuid::new_v4().to_string(),
            username,
            admin: false,
        };
        let result = sqlx::query("INSERT INTO users(id, username, password_hash) VALUES (?, ?, ?)")
            .bind(&user.id)
            .bind(&user.username)
            .bind(hash)
            .execute(&self.store.pool)
            .await;
        match result {
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                Err(Error::Conflict("That username is already in use".into()))
            }
            Err(e) => Err(e.into()),
            Ok(_) => {
                tracing::info!(account_id = %user.id, "Account created");
                Ok(user)
            }
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn users(&self, actor: &Actor) -> Result<Vec<User>> {
        actor.require_admin()?;
        Ok(
            sqlx::query_as("SELECT id, username, admin FROM users ORDER BY username LIMIT 1000")
                .fetch_all(&self.store.pool)
                .await?,
        )
    }
}

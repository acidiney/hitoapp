use super::{AuthSession, Credentials, Identity, User, credentials};
use crate::{Error, Result};
use uuid::Uuid;

impl Identity {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn needs_setup(&self) -> Result<bool> {
        Ok(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users")
            .fetch_one(&self.store.pool)
            .await?
            == 0)
    }

    pub async fn setup(&self, credentials: Credentials) -> Result<AuthSession> {
        self.setup_with_client(credentials, None).await
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn setup_with_client(
        &self,
        credentials: Credentials,
        client: Option<&str>,
    ) -> Result<AuthSession> {
        let username = credentials::validate(&credentials)?;
        let password_hash = credentials::hash(credentials.password).await?;
        let user = User {
            id: Uuid::new_v4().to_string(),
            username,
            admin: true,
            display_name: None,
            avatar_version: None,
        };
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&mut *tx)
            .await?;
        if count > 0 {
            return Err(Error::Conflict(
                "Hito is already configured. Sign in instead.".into(),
            ));
        }
        sqlx::query("INSERT INTO users(id, username, password_hash, admin) VALUES (?, ?, ?, 1)")
            .bind(&user.id)
            .bind(&user.username)
            .bind(password_hash)
            .execute(&mut *tx)
            .await?;
        let library = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO libraries(id, name) VALUES (?, 'Music library')")
            .bind(&library)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO library_members VALUES (?, ?)")
            .bind(library)
            .bind(&user.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(account_id = %user.id, "Administrator account created");
        self.create_session(user, None, client).await
    }
}

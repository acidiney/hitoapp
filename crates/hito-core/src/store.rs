use crate::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{path::Path, time::Duration};

#[derive(Clone)]
pub(crate) struct Store {
    pub(crate) pool: sqlx::SqlitePool,
}

impl Store {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(crate) async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub(crate) async fn healthy(&self) -> bool {
        let ready = sqlx::query("SELECT 1").execute(&self.pool).await.is_ok();
        tracing::debug!(ready, "Database health checked");
        ready
    }
}

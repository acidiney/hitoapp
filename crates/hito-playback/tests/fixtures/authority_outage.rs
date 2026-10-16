#![allow(dead_code)]
use std::path::Path;

pub struct Outage {
    pool: sqlx::SqlitePool,
    table: &'static str,
}

impl Outage {
    pub async fn start(directory: &Path) -> Self {
        Self::rename(directory, "sessions").await
    }

    pub async fn sources(directory: &Path) -> Self {
        Self::rename(directory, "sources").await
    }
    pub async fn recordings(directory: &Path) -> Self {
        Self::rename(directory, "recordings").await
    }

    async fn rename(directory: &Path, table: &'static str) -> Self {
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new().filename(directory.join("hito.db")),
        )
        .await
        .unwrap();
        sqlx::query(&format!(
            "ALTER TABLE {table} RENAME TO unavailable_{table}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        Self { pool, table }
    }

    pub async fn restore(self) {
        sqlx::query(&format!(
            "ALTER TABLE unavailable_{} RENAME TO {}",
            self.table, self.table
        ))
        .execute(&self.pool)
        .await
        .unwrap();
        self.pool.close().await;
    }

    pub async fn restore_revoked(self) {
        assert_eq!(self.table, "sessions");
        let mut transaction = self.pool.begin().await.unwrap();
        sqlx::query("DELETE FROM unavailable_sessions")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("ALTER TABLE unavailable_sessions RENAME TO sessions")
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        self.pool.close().await;
    }
}

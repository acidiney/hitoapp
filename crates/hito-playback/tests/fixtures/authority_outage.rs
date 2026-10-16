use std::path::Path;

pub struct Outage(sqlx::SqlitePool);

impl Outage {
    pub async fn start(directory: &Path) -> Self {
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new().filename(directory.join("hito.db")),
        )
        .await
        .unwrap();
        sqlx::query("ALTER TABLE sessions RENAME TO unavailable_sessions")
            .execute(&pool)
            .await
            .unwrap();
        Self(pool)
    }

    pub async fn restore(self) {
        sqlx::query("ALTER TABLE unavailable_sessions RENAME TO sessions")
            .execute(&self.0)
            .await
            .unwrap();
        self.0.close().await;
    }
}

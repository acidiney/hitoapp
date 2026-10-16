#![allow(dead_code)]
use std::path::Path;

pub struct SavedStore {
    pool: sqlx::SqlitePool,
}
impl SavedStore {
    pub async fn missing(directory: &Path) -> Self {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new().filename(directory.join("hito.db")),
            )
            .await
            .unwrap();
        sqlx::query("ALTER TABLE listening_progress RENAME TO saved_fixture_progress")
            .execute(&pool)
            .await
            .unwrap();
        Self { pool }
    }
    pub async fn delayed(directory: &Path) -> Self {
        let fixture = Self::missing(directory).await;
        sqlx::query("CREATE VIEW listening_progress AS WITH RECURSIVE delay(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM delay WHERE n<10000000) SELECT user_id, recording_id, position_ms + (SELECT sum(n)*0 FROM delay) AS position_ms, speed, updated_at FROM saved_fixture_progress").execute(&fixture.pool).await.unwrap();
        fixture
    }
    pub async fn restore(self) {
        sqlx::query("DROP VIEW IF EXISTS listening_progress")
            .execute(&self.pool)
            .await
            .unwrap();
        sqlx::query("ALTER TABLE saved_fixture_progress RENAME TO listening_progress")
            .execute(&self.pool)
            .await
            .unwrap();
        self.pool.close().await;
    }
}

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
        sqlx::query("CREATE TABLE saved_fixture_marks (mark INTEGER)")
            .execute(&pool)
            .await
            .unwrap();
        Self { pool }
    }
    pub async fn delayed(directory: &Path) -> Self {
        Self::delayed_iterations(directory, 10_000_000).await
    }
    pub async fn delayed_iterations(directory: &Path, iterations: u32) -> Self {
        let fixture = Self::missing(directory).await;
        sqlx::query(&format!("CREATE VIEW listening_progress AS WITH RECURSIVE delay(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM delay WHERE n<{iterations}) SELECT user_id, recording_id, position_ms + (SELECT sum(n)*0 FROM delay) AS position_ms, speed, updated_at FROM saved_fixture_progress")).execute(&fixture.pool).await.unwrap();
        fixture
    }
    pub async fn change(&self, position_ms: i64) {
        sqlx::query("UPDATE saved_fixture_progress SET position_ms = ?")
            .bind(position_ms)
            .execute(&self.pool)
            .await
            .unwrap();
    }
    pub async fn reading(&self) {
        sqlx::query("PRAGMA busy_timeout = 0")
            .execute(&self.pool)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if self.held_by_reader().await {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    if self.held_by_reader().await {
                        return;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the bounded fixture lookup must hold its read snapshot");
    }
    async fn held_by_reader(&self) -> bool {
        sqlx::query("INSERT INTO saved_fixture_marks DEFAULT VALUES")
            .execute(&self.pool)
            .await
            .unwrap();
        let (busy, _, _): (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(&self.pool)
            .await
            .unwrap();
        busy != 0
    }
    pub async fn restore(self) {
        sqlx::query("DROP TABLE saved_fixture_marks")
            .execute(&self.pool)
            .await
            .unwrap();
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

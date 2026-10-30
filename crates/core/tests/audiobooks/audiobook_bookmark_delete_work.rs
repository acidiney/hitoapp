#[path = "../fixtures/audiobook_bookmark_delete_work.rs"]
mod fixture;

use fixture::Statistics;
use sqlx::{Connection, SqliteConnection};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

const BUDGET: u64 = 50_000;
const ABORT_BUDGET: u64 = 20_000_000;

#[tokio::test]
async fn deleting_retired_follow_entries_does_not_scan_unrelated_private_bookmark_history() {
    for statistics in [Statistics::Absent, Statistics::Inherited, Statistics::Fresh] {
        let database = fixture::seed(statistics).await;
        assert!(
            database.directory.path().is_dir(),
            "The work fixture must own its disposable database"
        );
        let mut connection = SqliteConnection::connect_with(&database.options)
            .await
            .unwrap();
        fixture::assert_fixture(&mut connection).await;
        fixture::assert_statistics(&mut connection, statistics).await;
        let before = fixture::snapshot(&mut connection).await;
        connection.close().await.unwrap();

        let mut connection = SqliteConnection::connect_with(&database.options)
            .await
            .unwrap();
        fixture::assert_statistics(&mut connection, statistics).await;
        let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut connection)
            .await
            .unwrap();
        assert_eq!(
            enabled, 1,
            "The reopened measured connection must enforce foreign keys"
        );
        assert!(
            fixture::snapshot(&mut connection).await == before,
            "Closing and reopening must preserve the exact fixture and retained marker evidence"
        );
        let mut tx = connection.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let steps = Arc::new(AtomicU64::new(0));
        let progress = Arc::clone(&steps);
        tx.lock_handle()
            .await
            .unwrap()
            .set_progress_handler(100, move || {
                progress.fetch_add(100, Ordering::Relaxed) + 100 < ABORT_BUDGET
            });
        let started = Instant::now();
        let deleted =
            sqlx::query("DELETE FROM playlist_entries WHERE playlist_id = ? AND deleted = 1")
                .bind(fixture::FOLLOW)
                .execute(&mut *tx)
                .await;
        let elapsed = started.elapsed();
        tx.lock_handle().await.unwrap().remove_progress_handler();
        let work = steps.load(Ordering::Relaxed);
        let remaining = if deleted.is_ok() {
            Some(
                sqlx::query_scalar::<_, i64>(
                    "SELECT count(*) FROM playlist_entries WHERE playlist_id=? AND deleted=1",
                )
                .bind(fixture::FOLLOW)
                .fetch_one(&mut *tx)
                .await
                .unwrap(),
            )
        } else {
            None
        };
        let during = if deleted.is_ok() {
            Some(fixture::snapshot(&mut tx).await)
        } else {
            None
        };
        let rollback = tx.rollback().await;
        connection.close().await.unwrap();

        let mut restored = SqliteConnection::connect_with(&database.options)
            .await
            .unwrap();
        fixture::assert_fixture(&mut restored).await;
        fixture::assert_statistics(&mut restored, statistics).await;
        assert!(
            fixture::snapshot(&mut restored).await == before,
            "Rolling back the actual parent DELETE must retain every marker, owner, chapter and follow row exactly"
        );
        restored.close().await.unwrap();
        eprintln!(
            "Follow-entry DELETE statistics={statistics:?} retained_markers={} removed_entries={} VM_steps~{work} budget={BUDGET} elapsed={elapsed:?}",
            fixture::MARKERS,
            fixture::RETIRED_CHAPTERS
        );
        assert!(
            rollback.is_ok(),
            "The measured production write transaction must roll back cleanly"
        );
        assert!(
            deleted.is_ok(),
            "The actual parent DELETE must complete within the hard VM abort allowance"
        );
        assert_eq!(deleted.unwrap().rows_affected(), fixture::RETIRED_CHAPTERS);
        assert_eq!(
            remaining,
            Some(0),
            "The measured DELETE must actually remove all retired follow entries before rollback"
        );
        assert!(
            during
                .unwrap()
                .iter()
                .filter(|row| row.0 != "playlist_entries")
                .eq(before.iter().filter(|row| row.0 != "playlist_entries")),
            "The successful parent DELETE must preserve every unrelated marker and owner before rollback"
        );
        assert!(
            work < BUDGET,
            "Unrelated private bookmark history exceeded the parent DELETE work budget with {statistics:?}: steps~{work}, budget={BUDGET}"
        );
    }
}

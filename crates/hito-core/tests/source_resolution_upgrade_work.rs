use sqlx::{
    Connection, SqliteConnection,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use std::{
    borrow::Cow,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const SMALL: i64 = 256;
const LARGE: i64 = 4096;
const FIXED_WORK: u64 = 200_000;
const WORK_PER_BINDING: u64 = 2500;

#[tokio::test]
async fn rebuilding_provider_bindings_keeps_foreign_key_work_bounded_as_child_history_grows() {
    let directory = tempfile::tempdir().unwrap();
    let small = migrate(directory.path(), SMALL).await;
    let large = migrate(directory.path(), LARGE).await;
    assert!(
        large <= small * 24 + FIXED_WORK,
        "production binding upgrade work grew beyond the logarithmic index allowance: small={small}, large={large}"
    );
}

async fn migrate(directory: &Path, count: i64) -> u64 {
    let options = SqliteConnectOptions::new()
        .filename(directory.join(format!("bindings-{count}.db")))
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Delete);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 35)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&mut connection)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("fixtures/source_resolution_upgrade.sql"))
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("CREATE TEMP TABLE hito_resolution_work_numbers(i INTEGER PRIMARY KEY)")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<?) INSERT INTO hito_resolution_work_numbers SELECT i FROM n")
        .bind(count).execute(&mut connection).await.unwrap();
    let mut transaction = connection.begin().await.unwrap();
    sqlx::raw_sql(include_str!("fixtures/source_resolution_upgrade_work.sql"))
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    assert_fixture(&mut connection, count).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options.create_if_missing(false))
        .await
        .unwrap();
    assert_fixture(&mut connection, count).await;
    let budget = FIXED_WORK + (count as u64 + 3) * WORK_PER_BINDING;
    let steps = Arc::new(AtomicU64::new(0));
    let exceeded = Arc::new(AtomicBool::new(false));
    let progress = Arc::clone(&steps);
    let interrupted = Arc::clone(&exceeded);
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_progress_handler(1000, move || {
            let current = progress.fetch_add(1000, Ordering::Relaxed) + 1000;
            if current >= budget {
                interrupted.store(true, Ordering::Relaxed);
                false
            } else {
                true
            }
        });
    let measured = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 36)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    let result = measured.run(&mut connection).await;
    connection
        .lock_handle()
        .await
        .unwrap()
        .remove_progress_handler();
    let work = steps.load(Ordering::Relaxed);
    let aborted = exceeded.load(Ordering::Relaxed);
    if result.is_ok() {
        assert_fixture(&mut connection, count).await;
        let version: i64 =
            sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        assert_eq!(
            version, 36,
            "the measured operation must run actual production migration0036"
        );
    }
    connection.close().await.unwrap();
    eprintln!(
        "Provider binding upgrade rows={} VM_steps~{work} budget={budget} progress_aborted={aborted}",
        count + 3
    );
    assert!(
        !aborted,
        "production migration0036 exceeded its bounded foreign-key work allowance: bindings={}, steps~{work}, budget={budget}",
        count + 3
    );
    assert!(
        result.is_ok(),
        "production migration failed before its VM budget; this is not the expected performance regression"
    );
    assert!(work < budget);
    work
}

async fn assert_fixture(connection: &mut SqliteConnection, count: i64) {
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(
        enabled, 1,
        "the real migration must retain foreign-key enforcement"
    );
    let bindings: i64 = sqlx::query_scalar("SELECT count(*) FROM provider_items")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(bindings, count + 3);
    for (table, column) in [
        ("recordings", "metadata_binding_id"),
        ("artwork_fetches", "binding_id"),
        ("source_preparations", "binding_id"),
        ("download_intents", "binding_id"),
    ] {
        let references: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {table} WHERE {column} IS NOT NULL"
        ))
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        assert_eq!(
            references,
            count + 1,
            "each missing child key must have representative real rows: {table}.{column}"
        );
        let wrong: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table} child LEFT JOIN provider_items binding ON binding.id=child.{column} WHERE child.{column} IS NOT NULL AND binding.id IS NULL")).fetch_one(&mut *connection).await.unwrap();
        assert_eq!(wrong, 0);
    }
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty(),
        "the owned populated schema must have valid foreign keys before and after the real migration"
    );
}

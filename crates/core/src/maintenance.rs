use crate::{Error, Result, store::MIGRATOR};
use sqlx::{
    Connection, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteLockingMode},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tracing::{Instrument, instrument::WithSubscriber};

#[derive(Debug)]
pub struct MigrationOutcome {
    pub previous_version: i64,
    pub current_version: i64,
    pub applied_count: usize,
}

#[tracing::instrument(level = "info", skip_all, err(Debug), fields(maintenance_id = %uuid::Uuid::new_v4()))]
pub async fn migrate_existing(
    database: PathBuf,
    expected_version: i64,
) -> Result<MigrationOutcome> {
    tokio::spawn(
        migrate_owned(database, expected_version)
            .instrument(tracing::Span::current())
            .with_current_subscriber(),
    )
    .await
    .map_err(|_| Error::Internal("Database maintenance owner failed".into()))?
}

async fn migrate_owned(database: PathBuf, expected_version: i64) -> Result<MigrationOutcome> {
    let started = Instant::now();
    let metadata = tokio::fs::symlink_metadata(&database)
        .await
        .map_err(|_| Error::NotFound)?;
    if !metadata.is_file() {
        return Err(Error::Invalid(
            "Use an existing regular Hito database".into(),
        ));
    }
    let database = tokio::fs::canonicalize(database)
        .await
        .map_err(|_| Error::NotFound)?;
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(false)
            .foreign_keys(true)
            .locking_mode(SqliteLockingMode::Exclusive)
            .busy_timeout(Duration::from_secs(2)),
    )
    .await?;
    let result = migrate_on(&mut connection, expected_version).await;
    let closed = connection.close().await;
    tracing::info!(
        outcome = if result.is_ok() && closed.is_ok() {
            "complete"
        } else {
            "failed"
        },
        close_complete = closed.is_ok(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Database maintenance connection finished"
    );
    let outcome = result?;
    closed?;
    Ok(outcome)
}

async fn migrate_on(
    connection: &mut SqliteConnection,
    expected_version: i64,
) -> Result<MigrationOutcome> {
    let recognizable: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE type='table'
        AND name IN ('_sqlx_migrations','users','libraries','recordings','sync_events')",
    )
    .fetch_one(&mut *connection)
    .await?;
    if recognizable != 5 {
        return Err(Error::Invalid(
            "Use an existing Hito database with migration history".into(),
        ));
    }
    let applied: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut *connection)
            .await?;
    let previous_version = applied.last().copied().unwrap_or(0);
    if expected_version <= 0 || previous_version != expected_version {
        return Err(Error::Conflict(
            "The database migration version does not match the expected version".into(),
        ));
    }
    let expected: Vec<i64> = MIGRATOR
        .iter()
        .filter(|migration| migration.version <= expected_version)
        .map(|migration| migration.version)
        .collect();
    if expected.last() != Some(&expected_version) || applied != expected {
        return Err(Error::Invalid(
            "The database migration history is incomplete or unsupported".into(),
        ));
    }
    let invalid_success: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE typeof(success) <> 'integer' OR success <> 1)",
    )
    .fetch_one(&mut *connection)
    .await?;
    if invalid_success {
        return Err(Error::Invalid(
            "The database migration history contains invalid success values".into(),
        ));
    }
    MIGRATOR.run_direct(&mut *connection).await?;
    let current_version = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(connection)
        .await?;
    let outcome = MigrationOutcome {
        previous_version,
        current_version,
        applied_count: MIGRATOR.iter().count() - applied.len(),
    };
    tracing::info!(
        previous_version,
        current_version,
        applied_count = outcome.applied_count,
        "Embedded database migrations applied"
    );
    Ok(outcome)
}

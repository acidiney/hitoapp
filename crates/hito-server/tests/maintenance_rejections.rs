#[path = "fixtures/maintenance_rejections.rs"]
mod boundary;
#[path = "fixtures/maintenance_command.rs"]
mod command;
#[path = "fixtures/maintenance.rs"]
#[allow(dead_code)]
mod fixture;

use boundary::{TITLE, TOKEN, files, library, state};
use command::{run, run_at};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn a_missing_database_and_its_parent_are_never_created() {
    let directory = tempfile::Builder::new()
        .prefix("private-maintenance-path-")
        .tempdir()
        .unwrap();
    let database = directory.path().join("missing/PRIVATE_DATABASE_PATH.db");
    let before = files(directory.path(), false);
    let output = run(&database, 29).await;
    output.assert_failure(&database);
    assert!(
        !database.exists(),
        "a rejected missing database must remain absent"
    );
    assert!(
        files(directory.path(), false) == before,
        "rejection must not create a parent directory or application files"
    );
}

#[tokio::test]
async fn an_existing_empty_file_remains_empty_without_new_schema_or_files() {
    let directory = tempfile::Builder::new()
        .prefix("private-maintenance-path-")
        .tempdir()
        .unwrap();
    let database = directory.path().join("PRIVATE_DATABASE_PATH.db");
    std::fs::write(&database, []).unwrap();
    let before = files(directory.path(), false);
    let output = run(&database, 29).await;
    output.assert_failure(&database);
    assert!(
        files(directory.path(), false) == before,
        "rejection must preserve an empty file byte-for-byte and create no SQLite schema or sidecars"
    );
}

#[tokio::test]
async fn an_unrelated_sqlite_database_keeps_its_bytes_schema_and_private_values() {
    let directory = tempfile::Builder::new()
        .prefix("private-maintenance-path-")
        .tempdir()
        .unwrap();
    let database = directory.path().join("PRIVATE_DATABASE_PATH.db");
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&database)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query("CREATE TABLE unrelated(id INTEGER PRIMARY KEY,title TEXT,token BLOB,progress REAL,missing TEXT)")
        .execute(&mut connection).await.unwrap();
    sqlx::query("INSERT INTO unrelated(id,title,token,progress) VALUES (47,?,?,1.25)")
        .bind(TITLE)
        .bind(TOKEN.as_bytes())
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    let before_state = state(&database).await;
    let before = files(directory.path(), false);
    let output = run(&database, 29).await;
    output.assert_failure(&database);
    assert!(
        files(directory.path(), false) == before,
        "rejection must preserve unrelated SQLite bytes and file entries"
    );
    assert!(
        state(&database).await == before_state,
        "rejection must preserve unrelated schema and typed rows"
    );
}

#[tokio::test]
async fn a_wrong_expected_version_preserves_the_existing_hito_state() {
    let library = library().await;
    let before = state(&library.database).await;
    let retained = files(library.directory.path(), true);
    let output = run(&library.database, 28).await;
    output.assert_failure(&library.database);
    assert!(
        state(&library.database).await == before,
        "a version mismatch must leave every Hito table, typed row, rowid, and sequence unchanged"
    );
    assert!(
        files(library.directory.path(), true) == retained,
        "a version mismatch must preserve retained application files"
    );
}

#[tokio::test]
async fn repeating_migration_at_the_exact_latest_version_is_a_preserving_noop() {
    let library = library().await;
    let upgraded = run(&library.database, 29).await;
    let latest = fixture::migrations()
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap();
    let pending = fixture::migrations()
        .iter()
        .filter(|migration| migration.version > 29)
        .count();
    upgraded.assert_success(&library.database, 29, latest, pending as u64);
    let before = state(&library.database).await;
    let retained = files(library.directory.path(), true);
    let replay = run(&library.database, latest).await;
    replay.assert_success(&library.database, latest, latest, 0);
    assert!(
        state(&library.database).await == before,
        "an exact-version no-op must preserve all current schema, typed rows, rowids, and sequence values"
    );
    assert!(
        files(library.directory.path(), true) == retained,
        "an exact-version no-op must preserve retained files and create no application sidecars"
    );
}

#[tokio::test]
async fn a_checksum_mismatch_rejects_before_applying_new_migrations_without_private_output() {
    let library = library().await;
    let mut connection = fixture::connect(&library.database).await;
    sqlx::query("UPDATE _sqlx_migrations SET checksum=? WHERE version=29")
        .bind(TOKEN.as_bytes())
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    let before = state(&library.database).await;
    let retained = files(library.directory.path(), true);
    let output = run(&library.database, 29).await;
    output.assert_failure(&library.database);
    assert!(
        state(&library.database).await == before,
        "checksum rejection must preserve the original ledger and apply no migration or schema changes"
    );
    assert!(
        files(library.directory.path(), true) == retained,
        "checksum rejection must preserve retained application files"
    );
}

#[tokio::test]
async fn a_nonboolean_integer_success_value_rejects_before_pending_migrations() {
    let library = library().await;
    let mut connection = fixture::connect(&library.database).await;
    sqlx::query("UPDATE _sqlx_migrations SET success=? WHERE version=29")
        .bind(2_i64)
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    assert_corrupt_success_rejected(&library).await;
}

#[tokio::test]
async fn a_text_success_value_rejects_before_pending_migrations_without_private_output() {
    let library = library().await;
    let mut connection = fixture::connect(&library.database).await;
    sqlx::query("UPDATE _sqlx_migrations SET success=? WHERE version=29")
        .bind(TOKEN)
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    assert_corrupt_success_rejected(&library).await;
}

async fn assert_corrupt_success_rejected(library: &fixture::Library) {
    let before = state(&library.database).await;
    let retained = files(library.directory.path(), true);
    let output = run(&library.database, 29).await;
    output.assert_failure(&library.database);
    assert!(
        state(&library.database).await == before,
        "a noncanonical migration success value must be rejected without changing ledger types, rows, versions, or schema"
    );
    assert!(
        files(library.directory.path(), true) == retained,
        "a noncanonical migration ledger must preserve retained application files"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_uri_like_literal_filename_migrates_only_the_selected_existing_database() {
    let selected_source = library().await;
    let other_source = library().await;
    for (source, suffix) in [
        (&selected_source, "selected-literal"),
        (&other_source, "unselected"),
    ] {
        let mut connection = fixture::connect(&source.database).await;
        sqlx::query("UPDATE recordings SET title=?")
            .bind(format!("{TITLE}-{suffix}"))
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
    }
    let directory = tempfile::Builder::new()
        .prefix("private-maintenance-uri-")
        .tempdir()
        .unwrap();
    let selected = directory.path().join("file:hito.db");
    let other = directory.path().join("hito.db");
    std::fs::copy(&selected_source.database, &selected).unwrap();
    std::fs::copy(&other_source.database, &other).unwrap();
    let selected_before = fixture::Snapshot::read(&selected).await;
    let other_before = state(&other).await;
    let other_bytes = std::fs::read(&other).unwrap();
    assert!(
        other_bytes.len() <= 4194304,
        "owned database byte snapshot bound exceeded"
    );
    let names_before: Vec<_> = files(directory.path(), false).into_keys().collect();
    let output = run_at(&selected, 29, Some(directory.path())).await;
    let latest = fixture::migrations()
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap();
    let pending = fixture::migrations()
        .iter()
        .filter(|migration| migration.version > 29)
        .count();
    output.assert_success(&selected, 29, latest, pending as u64);
    assert!(
        state(&other).await == other_before,
        "maintenance migrated the unselected database while a URI-like literal filename was selected"
    );
    assert!(
        std::fs::read(&other).unwrap() == other_bytes,
        "maintenance changed the unselected database bytes"
    );
    selected_before.assert_preserved(&selected).await;
    let mut connection = fixture::connect(&selected).await;
    let version: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    assert_eq!(
        version, latest,
        "the selected literal filename must reach the actual compiled latest migration"
    );
    assert!(
        files(directory.path(), false)
            .into_keys()
            .collect::<Vec<_>>()
            == names_before,
        "migration must not create application files around the two owned databases"
    );
}

use sqlx::{Connection, Row, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::{borrow::Cow, path::Path};

#[path = "collection_artwork_schema.rs"]
mod collection_artwork_schema;

const LEDGER_SNAPSHOT: &str = "before_migration_ledger";

pub async fn connect(database: &Path) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(true)
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

pub async fn baseline(source: &Path, database: &Path) -> Vec<String> {
    let mut connection = connect(database).await;
    let migrations = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            migrations
                .iter()
                .filter(|migration| migration.version <= 27)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        27
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        1
    );
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_list WHERE schema = 'main' AND type = 'table' AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name")
        .fetch_all(&mut connection).await.unwrap();
    let triggers: Vec<(String, String)> =
        sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE type = 'trigger' ORDER BY name")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    sqlx::query("ATTACH DATABASE ? AS populated_source")
        .bind(source.to_string_lossy().as_ref())
        .execute(&mut connection)
        .await
        .unwrap();
    let mut tx = connection.begin().await.unwrap();
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *tx)
        .await
        .unwrap();
    for (name, _) in &triggers {
        sqlx::query(&format!("DROP TRIGGER {}", quoted(name)))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for table in &tables {
        let schema =
            sqlx::query("SELECT name,type,pk,hidden FROM pragma_table_xinfo(?) ORDER BY cid")
                .bind(table)
                .fetch_all(&mut *tx)
                .await
                .unwrap();
        let mut columns = schema
            .iter()
            .filter(|column| column.get::<i64, _>("hidden") == 0)
            .map(|column| quoted(column.get::<String, _>("name").as_str()))
            .collect::<Vec<_>>();
        let primary = schema
            .iter()
            .filter(|column| column.get::<i64, _>("pk") > 0)
            .collect::<Vec<_>>();
        if primary.len() != 1
            || !primary[0]
                .get::<String, _>("type")
                .eq_ignore_ascii_case("INTEGER")
        {
            columns.insert(0, "rowid".into());
        }
        let columns = columns.join(",");
        sqlx::query(&format!(
            "INSERT INTO {}({columns}) SELECT {columns} FROM populated_source.{} ORDER BY rowid",
            quoted(table),
            quoted(table)
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    sqlx::query(&format!(
        "CREATE TABLE {} AS SELECT rowid AS original_rowid,* FROM _sqlx_migrations",
        quoted(LEDGER_SNAPSHOT)
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("DELETE FROM sqlite_sequence WHERE name IN (SELECT name FROM populated_source.sqlite_sequence)").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO sqlite_sequence(name,seq) SELECT name,seq FROM populated_source.sqlite_sequence WHERE name IN (SELECT name FROM main.sqlite_schema WHERE type = 'table')")
        .execute(&mut *tx).await.unwrap();
    for (_, definition) in &triggers {
        sqlx::raw_sql(definition).execute(&mut *tx).await.unwrap();
    }
    let restored: Vec<(String, String)> =
        sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE type = 'trigger' ORDER BY name")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        restored, triggers,
        "Baseline must restore the exact original triggers"
    );
    for fts in [
        "recordings_fts",
        "artists_fts",
        "releases_fts",
        "personal_recordings_fts",
        "account_artists_fts",
        "account_releases_fts",
    ] {
        sqlx::query(&format!("INSERT INTO {fts}({fts}) VALUES ('rebuild')"))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for table in &tables {
        sqlx::query(&format!(
            "CREATE TABLE {} AS SELECT rowid AS original_rowid,* FROM {}",
            quoted(&format!("before_{table}")),
            quoted(table)
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *tx)
            .await
            .unwrap()
            .is_empty()
    );
    tx.commit().await.unwrap();
    sqlx::query("DETACH DATABASE populated_source")
        .execute(&mut connection)
        .await
        .unwrap();
    for fts in [
        "recordings_fts",
        "artists_fts",
        "releases_fts",
        "personal_recordings_fts",
        "account_artists_fts",
        "account_releases_fts",
    ] {
        sqlx::query(&format!(
            "INSERT INTO {fts}({fts},rank) VALUES ('integrity-check',1)"
        ))
        .execute(&mut connection)
        .await
        .unwrap();
    }
    let ledger: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name = 'sync_events'")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert!(
        !ledger.contains("playlist_follow"),
        "Baseline must have the actual old ledger constraint"
    );
    connection.close().await.unwrap();
    std::fs::copy(
        source.with_extension("plugin-key"),
        database.with_extension("plugin-key"),
    )
    .unwrap();
    tables
}

pub async fn assert_preserved(connection: &mut SqliteConnection, tables: &[String]) {
    for table in tables
        .iter()
        .map(String::as_str)
        .chain(std::iter::once("_sqlx_migrations"))
    {
        let snapshot = if table == "_sqlx_migrations" {
            LEDGER_SNAPSHOT.to_owned()
        } else {
            format!("before_{table}")
        };
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_xinfo(?) WHERE hidden=0 ORDER BY cid",
        )
        .bind(&snapshot)
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        assert_eq!(columns.first().map(String::as_str), Some("original_rowid"));
        let columns = &columns[1..];
        if table == "artwork_fetches" {
            assert_eq!(columns, collection_artwork_schema::LEGACY_JOB_COLUMNS);
        }
        if table == "_sqlx_migrations" {
            assert_eq!(
                columns,
                [
                    "version",
                    "description",
                    "installed_on",
                    "success",
                    "checksum",
                    "execution_time"
                ]
            );
        }
        let before = quoted(&snapshot);
        let table_name = quoted(table);
        let current = typed_projection("rowid", columns);
        let previous = typed_projection("original_rowid", columns);
        let filter = if table == "_sqlx_migrations" {
            " WHERE version <= 27"
        } else {
            ""
        };
        let differences: i64 = sqlx::query_scalar(&format!("SELECT
            (SELECT count(*) FROM (SELECT {current} FROM {table_name}{filter} EXCEPT SELECT {previous} FROM {before})) +
            (SELECT count(*) FROM (SELECT {previous} FROM {before} EXCEPT SELECT {current} FROM {table_name}{filter}))"))
            .fetch_one(&mut *connection).await.unwrap();
        assert_eq!(
            differences, 0,
            "The migration changed rows or rowids in {table}"
        );
    }
    for snapshot in tables
        .iter()
        .map(|table| format!("before_{table}"))
        .chain(std::iter::once(LEDGER_SNAPSHOT.to_owned()))
    {
        sqlx::query(&format!("DROP TABLE {}", quoted(&snapshot)))
            .execute(&mut *connection)
            .await
            .unwrap();
    }
    collection_artwork_schema::assert_current(connection).await;
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        "ok"
    );
    for fts in [
        "recordings_fts",
        "artists_fts",
        "releases_fts",
        "personal_recordings_fts",
        "account_artists_fts",
        "account_releases_fts",
    ] {
        sqlx::query(&format!(
            "INSERT INTO {fts}({fts},rank) VALUES ('integrity-check',1)"
        ))
        .execute(&mut *connection)
        .await
        .unwrap();
    }
}

fn typed_projection(rowid: &str, columns: &[String]) -> String {
    std::iter::once(quoted(rowid))
        .chain(columns.iter().map(|column| quoted(column)))
        .flat_map(|column| {
            [
                format!("{column} COLLATE BINARY"),
                format!("typeof({column})"),
            ]
        })
        .collect::<Vec<_>>()
        .join(",")
}

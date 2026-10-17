use sqlx::{
    Connection, Row, SqliteConnection, TypeInfo, ValueRef,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteRow},
};
use std::{borrow::Cow, collections::BTreeMap};

const LEGACY_VERSION: i64 = 43;

#[tokio::test]
async fn latest_migrator_preserves_closed_version_43_download_state() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("podcast-download-upgrade.db");
    let options = SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Delete);
    let all = sqlx::migrate!("./migrations");
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    through(&all, 35).run(&mut connection).await.unwrap();
    sqlx::raw_sql(include_str!("fixtures/source_resolution_upgrade.sql"))
        .execute(&mut connection)
        .await
        .unwrap();
    through(&all, LEGACY_VERSION)
        .run(&mut connection)
        .await
        .unwrap();
    assert_version(&mut connection, LEGACY_VERSION).await;
    assert_fixture(&mut connection).await;
    assert_integrity(&mut connection).await;
    let before = RetainedState::capture(&mut connection).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options.create_if_missing(false))
        .await
        .unwrap();
    assert_version(&mut connection, LEGACY_VERSION).await;
    before.assert_preserved(&mut connection).await;
    let expected = before.expecting_download_origins().await;
    all.run(&mut connection).await.unwrap();
    assert_version(&mut connection, 44).await;
    expected.assert_preserved(&mut connection).await;
    let origins = typed_rows(&mut connection,
        "SELECT origin_kind,podcast_source_id,podcast_episode_id,podcast_subscription_id,
            podcast_subscription_generation,podcast_enclosure_revision,podcast_enclosure_url FROM download_intents").await;
    assert!(
        origins
            == vec![vec![
                Cell::Text(b"provider".to_vec()),
                Cell::Null,
                Cell::Null,
                Cell::Null,
                Cell::Null,
                Cell::Null,
                Cell::Null
            ]],
        "the legacy provider origin must be backfilled without podcast witnesses"
    );
    assert_fixture(&mut connection).await;
    assert_integrity(&mut connection).await;

    let next_sequence: i64 = sqlx::query_scalar(
        "INSERT INTO metadata_observations(id,recording_id,provider,origin,title,artist,duration_ms)
         VALUES ('migration-after-podcast-download','migration-recording','owned','provider_response',
            'Owned after track','Owned artist',12000) RETURNING sequence",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert!(
        next_sequence > 900007,
        "the observation allocator must not reuse deleted history"
    );
    assert_integrity(&mut connection).await;
    connection.close().await.unwrap();
    assert!(database.is_file());
}

async fn assert_fixture(connection: &mut SqliteConnection) {
    for table in [
        "download_files",
        "download_intents",
        "download_jobs",
        "download_receipts",
        "download_retentions",
        "source_preparations",
        "source_preparation_receipts",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {}", quoted(table)))
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(count, 1, "the legacy fixture must populate {table}");
    }
    for (table, expected) in [
        ("recordings", vec![901, 1047]),
        ("provider_items", vec![507, 812, 1907]),
        ("sources", vec![102, 407, 901, 1231]),
        ("download_intents", vec![307]),
        ("source_preparations", vec![713]),
    ] {
        let rowids: Vec<i64> = sqlx::query_scalar(&format!(
            "SELECT rowid FROM {} ORDER BY rowid",
            quoted(table)
        ))
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        assert_eq!(
            rowids, expected,
            "the legacy fixture must retain {table} rowids"
        );
    }
    for view in [
        "current_artwork_fetch_authority",
        "current_download_authority",
        "current_source_preparation_authority",
        "download_file_origins",
        "retained_download_intents",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {}", quoted(view)))
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(
            count, 1,
            "the legacy fixture must have a live {view} witness"
        );
    }
    let high_water: i64 =
        sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name='metadata_observations'")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(high_water, 900007);
    let binding_index: String = sqlx::query_scalar(
        "SELECT tbl_name FROM sqlite_schema WHERE type='index' AND name='download_intent_binding'",
    )
    .fetch_one(connection)
    .await
    .unwrap();
    assert_eq!(binding_index, "download_intents");
}

type Rows = Vec<Vec<Cell>>;
type Schema = BTreeMap<(String, String), (String, Option<String>)>;
type Indexes = BTreeMap<(String, String), Rows>;

#[derive(PartialEq, Eq)]
enum Cell {
    Null,
    Integer(i64),
    Real(u64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

struct Projection {
    name: String,
    statement: String,
    rows: Rows,
}

struct RetainedState {
    schema: Schema,
    indexes: Indexes,
    projections: Vec<Projection>,
}

impl RetainedState {
    async fn capture(connection: &mut SqliteConnection) -> Self {
        let objects: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT schema.name,schema.type,metadata.wr FROM sqlite_schema schema
             JOIN pragma_table_list metadata ON metadata.schema='main' AND metadata.name=schema.name
             WHERE schema.type IN ('table','view') ORDER BY schema.type,schema.name",
        )
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        let mut projections = Vec::new();
        for (name, kind, without_rowid) in objects {
            let columns: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM pragma_table_xinfo(?) WHERE hidden!=1 ORDER BY cid",
            )
            .bind(&name)
            .fetch_all(&mut *connection)
            .await
            .unwrap();
            assert!(
                !columns.is_empty(),
                "capture every original column of {name}"
            );
            let columns = columns
                .iter()
                .map(|column| quoted(column))
                .collect::<Vec<_>>()
                .join(",");
            let has_rowid = kind == "table" && without_rowid == 0;
            let selected = if has_rowid {
                format!("rowid,{columns}")
            } else {
                columns.clone()
            };
            let order = if has_rowid { "rowid" } else { &columns };
            let filter = if name == "_sqlx_migrations" {
                format!(" WHERE version<={LEGACY_VERSION}")
            } else {
                String::new()
            };
            let statement = format!(
                "SELECT {selected} FROM {}{filter} ORDER BY {order}",
                quoted(&name)
            );
            projections.push(Projection {
                name,
                rows: typed_rows(connection, &statement).await,
                statement,
            });
        }
        Self {
            schema: schema(connection).await,
            indexes: indexes(connection).await,
            projections,
        }
    }

    async fn assert_preserved(&self, connection: &mut SqliteConnection) {
        assert!(
            schema(connection).await == self.schema,
            "the current migrator must retain the exact baseline schema definitions"
        );
        assert!(
            indexes(connection).await == self.indexes,
            "every legacy index definition, key, collation and uniqueness rule must survive"
        );
        for projection in &self.projections {
            assert!(
                typed_rows(connection, &projection.statement).await == projection.rows,
                "typed original columns, rowids, sequences, checksums or view witnesses changed in {}",
                projection.name
            );
        }
    }

    async fn expecting_download_origins(mut self) -> Self {
        let migration = include_str!("../migrations/0044_podcast_download_origins.sql");
        let mut reference = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        let table = migration
            .split("CREATE TABLE download_intents_next (")
            .nth(1)
            .unwrap()
            .split("\nINSERT INTO download_intents_next")
            .next()
            .unwrap();
        sqlx::raw_sql(&format!("CREATE TABLE download_intents_next ({table}"))
            .execute(&mut reference)
            .await
            .unwrap();
        sqlx::query("ALTER TABLE download_intents_next RENAME TO download_intents")
            .execute(&mut reference)
            .await
            .unwrap();
        for (kind, name, terminator) in [
            ("TRIGGER", "download_intent_identity", "END;"),
            ("VIEW", "current_download_authority", ";"),
            ("VIEW", "download_file_origins", ";"),
        ] {
            let prefix = format!("CREATE {kind} {name} ");
            let body = migration
                .split(&prefix)
                .nth(1)
                .unwrap()
                .split(terminator)
                .next()
                .unwrap();
            sqlx::raw_sql(&format!("{prefix}{body}{terminator}"))
                .execute(&mut reference)
                .await
                .unwrap();
        }
        let declared = schema(&mut reference).await;
        for (kind, name) in [
            ("table", "download_intents"),
            ("trigger", "download_intent_identity"),
            ("view", "current_download_authority"),
            ("view", "download_file_origins"),
        ] {
            let key = (kind.to_owned(), name.to_owned());
            assert!(self.schema.contains_key(&key));
            self.schema
                .insert(key.clone(), declared.get(&key).unwrap().clone());
        }
        reference.close().await.unwrap();
        self
    }
}

async fn schema(connection: &mut SqliteConnection) -> Schema {
    let definitions: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT type,name,tbl_name,sql FROM sqlite_schema
         WHERE type IN ('table','view','trigger','index') ORDER BY type,name",
    )
    .fetch_all(connection)
    .await
    .unwrap();
    definitions
        .into_iter()
        .map(|(kind, name, table, definition)| ((kind, name), (table, definition)))
        .collect()
}

async fn indexes(connection: &mut SqliteConnection) -> Indexes {
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    let mut retained = BTreeMap::new();
    for table in tables {
        let listed = sqlx::query(
            "SELECT name,\"unique\",origin,partial FROM pragma_index_list(?) ORDER BY name",
        )
        .bind(&table)
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        for index in listed {
            let name: String = index.get("name");
            let mut rows = vec![cells(&index)];
            rows.extend(
                sqlx::query("SELECT * FROM pragma_index_xinfo(?) ORDER BY seqno")
                    .bind(&name)
                    .fetch_all(&mut *connection)
                    .await
                    .unwrap()
                    .iter()
                    .map(cells),
            );
            retained.insert((table.clone(), name), rows);
        }
    }
    retained
}

async fn typed_rows(connection: &mut SqliteConnection, statement: &str) -> Rows {
    sqlx::query(statement)
        .fetch_all(connection)
        .await
        .unwrap()
        .iter()
        .map(cells)
        .collect()
}

fn cells(row: &SqliteRow) -> Vec<Cell> {
    (0..row.columns().len())
        .map(|index| {
            let value = row.try_get_raw(index).unwrap();
            if value.is_null() {
                return Cell::Null;
            }
            match value.type_info().name() {
                "INTEGER" | "BOOLEAN" => Cell::Integer(row.try_get(index).unwrap()),
                "REAL" => Cell::Real(row.try_get::<f64, _>(index).unwrap().to_bits()),
                "TEXT" => Cell::Text(row.try_get::<String, _>(index).unwrap().into_bytes()),
                "BLOB" => Cell::Blob(row.try_get(index).unwrap()),
                _ => panic!("the owned migration fixture has an unsupported SQLite type"),
            }
        })
        .collect()
}

fn through(all: &Migrator, version: i64) -> Migrator {
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= version)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
}

async fn assert_version(connection: &mut SqliteConnection, expected: i64) {
    let version: (i64, i64) =
        sqlx::query_as("SELECT max(version),count(*) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(connection)
            .await
            .unwrap();
    assert_eq!(version, (expected, expected));
}

async fn assert_integrity(connection: &mut SqliteConnection) {
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty(),
        "foreign key integrity must hold"
    );
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_schema WHERE type='table' AND sql LIKE '%USING fts5%' ORDER BY name",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    assert_eq!(tables.len(), 6);
    for table in tables {
        sqlx::query(&format!(
            "INSERT INTO {}({},rank) VALUES ('integrity-check',1)",
            quoted(&table),
            quoted(&table)
        ))
        .execute(&mut *connection)
        .await
        .unwrap();
    }
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

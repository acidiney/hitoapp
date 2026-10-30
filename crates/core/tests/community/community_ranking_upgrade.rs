#[path = "../fixtures/collection_artwork_schema.rs"]
mod historical_schema;
use hito_core::test_support::support;

use sqlx::{
    Connection, Row, SqliteConnection, TypeInfo, ValueRef, migrate::Migrator,
    sqlite::SqliteConnectOptions,
};
use std::{borrow::Cow, time::Duration};

type Column = (i64, String, String, i64, Option<String>, i64, i64);
type ForeignKey = (i64, i64, String, String, String, String, String, String);

#[tokio::test]
async fn ranking_upgrade_preserves_existing_52_state_without_consent() {
    tokio::time::timeout(Duration::from_secs(20), exercise())
        .await
        .expect("The owned historical migration must finish within its deadline");
}

async fn exercise() {
    let directory = tempfile::tempdir().unwrap();
    let original_path = directory.path().join("private-original.wav");
    let original = support::write_wav(&original_path, 14);
    let original_metadata = std::fs::metadata(&original_path).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("historical.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut database = SqliteConnection::connect_with(&options).await.unwrap();
    let all = sqlx::migrate!("./migrations");
    through(&all, 35).run(&mut database).await.unwrap();
    sqlx::raw_sql(include_str!("../fixtures/source_resolution_upgrade.sql"))
        .execute(&mut database)
        .await
        .unwrap();
    through(&all, 52).run(&mut database).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO account_community_preferences(rowid,user_id,revision,playback_reporting,metadata_corrections) VALUES
            (17,'migration-owner',7,1,0),(83,'migration-other',4,0,1);
         INSERT INTO community_reporters(rowid,user_id,service_key,reporter_id,credential) VALUES
            (31,'migration-owner',printf('%064d',1),'10000000-0000-4000-8000-000000000001',zeroblob(137)),
            (307,'migration-other',printf('%064d',2),'10000000-0000-4000-8000-000000000002',zeroblob(137));
         INSERT INTO community_reports(rowid,user_id,submission_id,service_key,session_hash,source_id,provider,public_recording_id,status,attempts,created_at,updated_at) VALUES
            (101,'migration-owner','20000000-0000-4000-8000-000000000001',printf('%064d',1),'owned-migration-session','migration-source-manual','youtube','RankVideo01','accepted',1,123,124),
            (701,'migration-owner','20000000-0000-4000-8000-000000000002',printf('%064d',1),'owned-migration-session','migration-source-manual','jamendo','42','queued',0,125,126),
            (1907,'migration-other','20000000-0000-4000-8000-000000000003',printf('%064d',2),'retained-owned-peer-session','migration-source-retired','jamendo','43','cancelled',2,127,128);
         INSERT INTO source_choice_receipts(rowid,user_id,request_id,recording_id,source_id,revision) VALUES
            (53,'migration-owner','private-choice-request','migration-recording','migration-source-manual',2);
         INSERT INTO source_choice_reset_receipts(rowid,user_id,request_id,recording_id,expected_revision,revision) VALUES
            (407,'migration-owner','private-reset-request','migration-recording',2,3);",
    )
    .execute(&mut database)
    .await
    .unwrap();
    let version: (i64, i64) =
        sqlx::query_as("SELECT max(version),count(*) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&mut database)
            .await
            .unwrap();
    assert_eq!(version, (52, 52));
    let mut reference_52 = historical_schema::reference_52().await;
    let before_schema = historical_schema::schema(&mut database).await;
    assert!(before_schema == historical_schema::schema(&mut reference_52).await);
    reference_52.close().await.unwrap();
    let before = capture(&mut database).await;
    assert!(before.iter().any(|projection| projection.table == "community_reports" && projection.rows.len() == 3));
    assert!(
        before
            .iter()
            .any(|projection| projection.table == "community_reporters"
                && projection.rows.len() == 2)
    );
    assert!(before.iter().any(|projection| projection.table == "_sqlx_migrations" && projection.rows.len() == 52));
    integrity(&mut database).await;
    database.close().await.unwrap();

    let mut database = SqliteConnection::connect_with(&options).await.unwrap();
    through(&all, 53).run(&mut database).await.unwrap();
    for projection in &before {
        assert!(
            typed_rows(&mut database, &projection.statement).await == projection.rows,
            "Every historical typed value, generated column, rowid, credential, asset and receipt must survive in {}",
            projection.table
        );
    }
    let after_schema = historical_schema::schema(&mut database).await;
    for old in &before_schema {
        assert!(
            after_schema.iter().any(|current| current == old),
            "Migration 53 must not remove or replace any historical schema object"
        );
    }
    let added = after_schema
        .iter()
        .filter(|object| {
            !before_schema
                .iter()
                .any(|old| old.0 == object.0 && old.1 == object.1)
        })
        .cloned()
        .collect::<Vec<_>>();
    let table = "account_community_ranking_preferences";
    assert_eq!(
        added,
        vec![
            (
                "index".into(),
                "sqlite_autoindex_account_community_ranking_preferences_1".into(),
                table.into(),
                None,
            ),
            (
                "table".into(),
                table.into(),
                table.into(),
                Some(
                    include_str!("../../migrations/0053_community_rankings.sql")
                        .trim()
                        .trim_end_matches(';')
                        .into()
                ),
            ),
        ],
        "Only the named ranking preference table and its primary key index may be added"
    );
    let mut current_reference = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    through(&all, 53).run(&mut current_reference).await.unwrap();
    assert!(after_schema == historical_schema::schema(&mut current_reference).await);
    current_reference.close().await.unwrap();
    let columns: Vec<Column> = sqlx::query_as("SELECT cid,name,type,\"notnull\",dflt_value,pk,hidden FROM pragma_table_xinfo(?) ORDER BY cid")
        .bind(table).fetch_all(&mut database).await.unwrap();
    assert_eq!(
        columns,
        vec![
            (0, "user_id".into(), "TEXT".into(), 0, None, 1, 0),
            (1, "revision".into(), "INTEGER".into(), 1, None, 0, 0),
            (
                2,
                "use_source_rankings".into(),
                "INTEGER".into(),
                1,
                None,
                0,
                0
            ),
        ]
    );
    let foreign_keys: Vec<ForeignKey> = sqlx::query_as("SELECT id,seq,\"table\",\"from\",\"to\",on_update,on_delete,\"match\" FROM pragma_foreign_key_list(?) ORDER BY id,seq")
        .bind(table).fetch_all(&mut database).await.unwrap();
    assert_eq!(
        foreign_keys,
        vec![(
            0,
            0,
            "users".into(),
            "user_id".into(),
            "id".into(),
            "NO ACTION".into(),
            "NO ACTION".into(),
            "NONE".into()
        )]
    );
    let invented: i64 =
        sqlx::query_scalar("SELECT count(*) FROM account_community_ranking_preferences")
            .fetch_one(&mut database)
            .await
            .unwrap();
    assert_eq!(
        invented, 0,
        "Existing reporting/correction consent must not enable receiving rankings"
    );
    let ledger: Vec<(i64, String, bool, Vec<u8>)> = sqlx::query_as(
        "SELECT version,description,success,checksum FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&mut database)
    .await
    .unwrap();
    let expected = all
        .iter()
        .filter(|migration| migration.version <= 53)
        .map(|migration| {
            (
                migration.version,
                migration.description.to_string(),
                true,
                migration.checksum.to_vec(),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        ledger == expected,
        "All 53 migration identities, checksums and successful receipts must match production history"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE version>52")
        .fetch_one(&mut database)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let high_water: i64 =
        sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name='metadata_observations'")
            .fetch_one(&mut database)
            .await
            .unwrap();
    assert_eq!(high_water, 900007);
    for statement in [
        "INSERT INTO account_community_ranking_preferences VALUES ('migration-owner',0,0)",
        "INSERT INTO account_community_ranking_preferences VALUES ('migration-owner',1,2)",
        "INSERT INTO account_community_ranking_preferences VALUES ('missing-owned-account',1,0)",
    ] {
        assert!(
            sqlx::query(statement).execute(&mut database).await.is_err(),
            "Ranking preferences must retain their revision, boolean and account constraints"
        );
    }
    integrity(&mut database).await;
    database.close().await.unwrap();
    assert!(std::fs::read(&original_path).unwrap() == original);
    let current = std::fs::metadata(&original_path).unwrap();
    assert_eq!(current.len(), original_metadata.len());
    assert_eq!(
        current.modified().unwrap(),
        original_metadata.modified().unwrap()
    );
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

#[derive(PartialEq, Eq)]
enum Cell {
    Null,
    Integer(i64),
    Real(u64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

struct Projection {
    table: String,
    statement: String,
    rows: Vec<Vec<Cell>>,
}

async fn capture(connection: &mut SqliteConnection) -> Vec<Projection> {
    let tables: Vec<(String,i64)> = sqlx::query_as("SELECT s.name,t.wr FROM sqlite_schema s JOIN pragma_table_list t ON t.schema='main' AND t.name=s.name WHERE s.type='table' ORDER BY s.name")
        .fetch_all(&mut *connection).await.unwrap();
    let mut projections = Vec::new();
    for (table, without_rowid) in tables {
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_xinfo(?) WHERE hidden!=1 ORDER BY cid",
        )
        .bind(&table)
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        let columns = columns
            .iter()
            .map(|name| quoted(name))
            .collect::<Vec<_>>()
            .join(",");
        let selected = if without_rowid == 0 {
            format!("rowid,{columns}")
        } else {
            columns.clone()
        };
        let order = if without_rowid == 0 {
            "rowid"
        } else {
            &columns
        };
        let filter = if table == "_sqlx_migrations" {
            " WHERE version<=52"
        } else {
            ""
        };
        let statement = format!(
            "SELECT {selected} FROM {}{filter} ORDER BY {order}",
            quoted(&table)
        );
        let rows = typed_rows(connection, &statement).await;
        projections.push(Projection {
            table,
            statement,
            rows,
        });
    }
    projections
}

async fn typed_rows(connection: &mut SqliteConnection, statement: &str) -> Vec<Vec<Cell>> {
    sqlx::query(statement)
        .fetch_all(connection)
        .await
        .unwrap()
        .iter()
        .map(|row| {
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
                        _ => {
                            panic!("The owned migration fixture must have an explicit SQLite type")
                        }
                    }
                })
                .collect()
        })
        .collect()
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

async fn integrity(connection: &mut SqliteConnection) {
    let result: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(result, "ok");
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(connection)
            .await
            .unwrap()
            .is_empty()
    );
}

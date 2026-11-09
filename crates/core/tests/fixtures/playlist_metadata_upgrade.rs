use futures_util::TryStreamExt;
use sha2::{Digest, Sha256};
use sqlx::{
    Connection, Row, SqliteConnection, TypeInfo, ValueRef, migrate::Migrator,
    sqlite::SqliteConnectOptions,
};
use std::{borrow::Cow, path::Path};

#[path = "collection_artwork_schema.rs"]
pub mod collection_artwork_schema;
#[path = "playlist_metadata_schema.rs"]
mod playlist_metadata_schema;

type SchemaObject = (String, String, String, Option<String>);
type TableDigest = (String, Vec<String>, u64, [u8; 32]);

pub struct Baseline {
    tables: Vec<String>,
    rows: Vec<TableDigest>,
    schema: Vec<SchemaObject>,
}

pub async fn connect(path: &Path) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

async fn schema(connection: &mut SqliteConnection) -> Vec<SchemaObject> {
    sqlx::query_as("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
        .fetch_all(connection)
        .await
        .unwrap()
}

struct TableLayout {
    without_rowid: bool,
    order: String,
}

async fn layout(connection: &mut SqliteConnection, table: &str) -> TableLayout {
    let without_rowid: i64 =
        sqlx::query_scalar("SELECT wr FROM pragma_table_list WHERE schema = 'main' AND name = ?")
            .bind(table)
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    let order = if without_rowid == 1 {
        let primary: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_xinfo(?) WHERE pk > 0 ORDER BY pk")
                .bind(table)
                .fetch_all(connection)
                .await
                .unwrap();
        assert!(
            !primary.is_empty(),
            "WITHOUT ROWID tables require a primary key"
        );
        primary
            .iter()
            .map(|name| quoted(name))
            .collect::<Vec<_>>()
            .join(",")
    } else {
        "rowid".into()
    };
    TableLayout {
        without_rowid: without_rowid == 1,
        order,
    }
}

async fn rows(connection: &mut SqliteConnection, tables: &[String]) -> Vec<TableDigest> {
    let mut result = Vec::new();
    for table in tables {
        let mut columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_xinfo(?) ORDER BY cid")
                .bind(table)
                .fetch_all(&mut *connection)
                .await
                .unwrap();
        let selected = if table == "artwork_fetches" {
            let legacy = collection_artwork_schema::LEGACY_JOB_COLUMNS
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>();
            let extended = collection_artwork_schema::LEGACY_JOB_COLUMNS
                .iter()
                .chain(collection_artwork_schema::NEW_JOB_COLUMNS)
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>();
            assert!(
                columns == legacy || columns == extended,
                "only the exact declared collection-target columns may extend historical artwork jobs"
            );
            columns = legacy;
            columns
                .iter()
                .map(|name| quoted(name))
                .collect::<Vec<_>>()
                .join(",")
        } else if table == "playlist_follows" {
            columns.retain(|name| {
                !matches!(
                    name.as_str(),
                    "approved_capabilities" | "approved_credential"
                )
            });
            columns
                .iter()
                .map(|name| quoted(name))
                .collect::<Vec<_>>()
                .join(",")
        } else if table == "plugin_authorization_attempts" {
            columns.retain(|name| name != "status" && name != "delivery");
            columns
                .iter()
                .map(|name| quoted(name))
                .collect::<Vec<_>>()
                .join(",")
        } else if table == "users" {
            columns.retain(|name| name != "display_name");
            columns
                .iter()
                .map(|name| quoted(name))
                .collect::<Vec<_>>()
                .join(",")
        } else if table == "sessions" {
            columns
                .retain(|name| !matches!(name.as_str(), "created_at" | "last_seen_at" | "client"));
            columns
                .iter()
                .map(|name| quoted(name))
                .collect::<Vec<_>>()
                .join(",")
        } else {
            "*".into()
        };
        let layout = layout(connection, table).await;
        let filter = if table == "_sqlx_migrations" {
            " WHERE version <= 45"
        } else {
            ""
        };
        let identity = if layout.without_rowid { "" } else { "rowid," };
        let query = format!(
            "SELECT {identity}{selected} FROM {}{filter} ORDER BY {}",
            quoted(table),
            layout.order
        );
        let mut stream = sqlx::query(&query).fetch(&mut *connection);
        let mut digest = Sha256::new();
        let mut count = 0;
        while let Some(row) = stream.try_next().await.unwrap() {
            digest.update((row.len() as u64).to_le_bytes());
            for index in 0..row.len() {
                let raw = row.try_get_raw(index).unwrap();
                let kind = raw.type_info();
                let value = match kind.name() {
                    "NULL" => Vec::new(),
                    "INTEGER" => row.get::<i64, _>(index).to_le_bytes().to_vec(),
                    "REAL" => row.get::<f64, _>(index).to_bits().to_le_bytes().to_vec(),
                    "TEXT" => row.get::<String, _>(index).into_bytes(),
                    "BLOB" => row.get::<Vec<u8>, _>(index),
                    _ => {
                        panic!("The migration fixture encountered an unsupported SQLite value type")
                    }
                };
                digest.update((kind.name().len() as u64).to_le_bytes());
                digest.update(kind.name().as_bytes());
                digest.update((value.len() as u64).to_le_bytes());
                digest.update(value);
            }
            count += 1;
        }
        result.push((table.clone(), columns, count, digest.finalize().into()));
    }
    result
}

pub async fn historical_45(source: &Path, destination: &Path) -> Baseline {
    let mut connection = connect(destination).await;
    let migrations = sqlx::migrate!("./migrations");
    let historical = Migrator {
        migrations: Cow::Owned(
            migrations
                .iter()
                .filter(|migration| migration.version <= 45)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    historical.run(&mut connection).await.unwrap();
    assert_eq!(version(&mut connection).await, 45);
    let ledger: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version,checksum,success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(ledger.len(), historical.iter().count());
    for ((version, checksum, success), migration) in ledger.iter().zip(historical.iter()) {
        assert_eq!(*version, migration.version);
        assert!(checksum.as_slice() == migration.checksum.as_ref() && *success);
    }
    let original_schema = schema(&mut connection).await;
    let ledger_sql = original_schema
        .iter()
        .find(|object| object.1 == "sync_events")
        .unwrap()
        .3
        .as_ref()
        .unwrap();
    assert!(!ledger_sql.contains("playlist_metadata"));
    assert!(
        !original_schema
            .iter()
            .any(|object| object.1 == "playlist_metadata_refresh_receipts")
    );
    let mut tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_table_list WHERE schema = 'main' AND type IN ('table','shadow')
         AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    let copied: Vec<String> = tables
        .iter()
        .filter(|table| table.as_str() != "_sqlx_migrations")
        .cloned()
        .collect();
    let mut copied_state = copied.clone();
    copied_state.push("sqlite_sequence".into());
    let mut source_connection = connect(source).await;
    assert_eq!(
        version(&mut source_connection).await,
        collection_artwork_schema::latest_version()
    );
    collection_artwork_schema::assert_current(&mut source_connection).await;
    let source_ledger: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as(
        "SELECT version,checksum,success FROM _sqlx_migrations WHERE version <= 45 ORDER BY version",
    ).fetch_all(&mut source_connection).await.unwrap();
    assert!(
        source_ledger == ledger,
        "The public seed must use the same historical migration checksums"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM playlist_metadata_refresh_receipts")
            .fetch_one(&mut source_connection)
            .await
            .unwrap(),
        0
    );
    let source_rows = rows(&mut source_connection, &copied_state).await;
    source_connection.close().await.unwrap();
    sqlx::query("ATTACH DATABASE ? AS public_seed")
        .bind(source.to_str().unwrap())
        .execute(&mut connection)
        .await
        .unwrap();
    let mut tx = connection.begin().await.unwrap();
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *tx)
        .await
        .unwrap();
    for object in &original_schema {
        if object.0 == "trigger" {
            sqlx::query(&format!("DROP TRIGGER {}", quoted(&object.1)))
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    for table in &copied {
        sqlx::query(&format!("DELETE FROM main.{}", quoted(table)))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for table in &copied {
        let layout = layout(&mut tx, table).await;
        let columns =
            sqlx::query("SELECT name,type,pk,hidden FROM pragma_table_xinfo(?) ORDER BY cid")
                .bind(table)
                .fetch_all(&mut *tx)
                .await
                .unwrap();
        let mut names: Vec<String> = columns
            .iter()
            .filter(|column| column.get::<i64, _>("hidden") == 0)
            .map(|column| quoted(column.get::<String, _>("name").as_str()))
            .collect();
        let primary: Vec<_> = columns
            .iter()
            .filter(|column| column.get::<i64, _>("pk") > 0)
            .collect();
        if !layout.without_rowid
            && (primary.len() != 1
                || !primary[0]
                    .get::<String, _>("type")
                    .eq_ignore_ascii_case("INTEGER"))
        {
            names.insert(0, "rowid".into());
        }
        let names = names.join(",");
        sqlx::query(&format!(
            "INSERT INTO {}({names}) SELECT {names} FROM public_seed.{} ORDER BY {}",
            quoted(table),
            quoted(table),
            layout.order
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    sqlx::query("DELETE FROM sqlite_sequence")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sqlite_sequence(rowid,name,seq) SELECT rowid,name,seq FROM public_seed.sqlite_sequence
        WHERE name IN (SELECT name FROM main.sqlite_schema WHERE type = 'table') ORDER BY rowid",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    for object in &original_schema {
        if object.0 == "trigger" {
            sqlx::raw_sql(object.3.as_ref().unwrap())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    tx.commit().await.unwrap();
    sqlx::query("DETACH DATABASE public_seed")
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    let mut connection = connect(destination).await;
    assert!(
        schema(&mut connection).await == original_schema,
        "Copying public state must retain the exact historical schema"
    );
    assert!(
        rows(&mut connection, &copied_state).await == source_rows,
        "The actual schema-45 baseline must retain the public seed's typed rows, application rowids and sequence high-water"
    );
    tables.push("sqlite_sequence".into());
    let baseline = Baseline {
        rows: Vec::new(),
        tables,
        schema: original_schema,
    };
    let baseline = checkpoint_45(&mut connection, &baseline).await;
    connection.close().await.unwrap();
    std::fs::copy(
        source.with_extension("plugin-key"),
        destination.with_extension("plugin-key"),
    )
    .unwrap();
    baseline
}

pub async fn assert_preserved(
    connection: &mut SqliteConnection,
    baseline: &Baseline,
    expected_version: i64,
) {
    assert!(
        rows(connection, &baseline.tables).await == baseline.rows,
        "Migration 0046 must preserve exact typed rows, application rowids, FTS shadow bytes and the prior ledger"
    );
    let after = schema(connection).await;
    let mut reference = playlist_metadata_schema::reference().await;
    let declared = schema(&mut reference).await;
    let sync_events = declared
        .iter()
        .find(|object| object.1 == "sync_events")
        .unwrap();
    let artwork = if matches!(expected_version, 52..=90) {
        let mut reference = if expected_version == 52 {
            collection_artwork_schema::reference_52().await
        } else if expected_version == 53 {
            collection_artwork_schema::reference_53().await
        } else if expected_version == 54 {
            collection_artwork_schema::reference().await
        } else if expected_version == 55 {
            collection_artwork_schema::reference_55().await
        } else if expected_version == 56 {
            collection_artwork_schema::reference_56().await
        } else if expected_version == 57 {
            collection_artwork_schema::reference_57().await
        } else if expected_version == 58 {
            collection_artwork_schema::reference_58().await
        } else if expected_version == 59 {
            collection_artwork_schema::reference_59().await
        } else if expected_version == 60 {
            collection_artwork_schema::reference_60().await
        } else if expected_version == 61 {
            collection_artwork_schema::reference_61().await
        } else if expected_version == 62 {
            collection_artwork_schema::reference_62().await
        } else if expected_version == 63 {
            collection_artwork_schema::reference_63().await
        } else if expected_version == 64 {
            collection_artwork_schema::reference_64().await
        } else if expected_version == 65 {
            collection_artwork_schema::reference_65().await
        } else if expected_version == 66 {
            collection_artwork_schema::reference_66().await
        } else if expected_version == 67 {
            collection_artwork_schema::reference_67().await
        } else if expected_version == 68 {
            collection_artwork_schema::reference_68().await
        } else if expected_version == 69 {
            collection_artwork_schema::reference_69().await
        } else if expected_version == 70 {
            collection_artwork_schema::reference_70().await
        } else if expected_version == 71 {
            collection_artwork_schema::reference_71().await
        } else if expected_version == 72 {
            collection_artwork_schema::reference_72().await
        } else if expected_version == 73 {
            collection_artwork_schema::reference_73().await
        } else if expected_version == 74 {
            collection_artwork_schema::reference_74().await
        } else if expected_version == 75 {
            collection_artwork_schema::reference_75().await
        } else if expected_version == 76 {
            collection_artwork_schema::reference_76().await
        } else if expected_version == 77 {
            collection_artwork_schema::reference_77().await
        } else if expected_version == 78 {
            collection_artwork_schema::reference_78().await
        } else if expected_version == 79 {
            collection_artwork_schema::reference_79().await
        } else if expected_version == 80 {
            collection_artwork_schema::reference_80().await
        } else if expected_version == 81 {
            collection_artwork_schema::reference_81().await
        } else if expected_version == 82 {
            collection_artwork_schema::reference_82().await
        } else if expected_version == 83 {
            collection_artwork_schema::reference_83().await
        } else if expected_version == 84 {
            collection_artwork_schema::reference_84().await
        } else if expected_version == 85 {
            collection_artwork_schema::reference_85().await
        } else if expected_version == 86 {
            collection_artwork_schema::reference_86().await
        } else if expected_version == 87 {
            collection_artwork_schema::reference_87().await
        } else if expected_version == 88 {
            collection_artwork_schema::reference_88().await
        } else if expected_version == 89 {
            collection_artwork_schema::reference_89().await
        } else {
            collection_artwork_schema::reference_90().await
        };
        let declared = schema(&mut reference).await;
        reference.close().await.unwrap();
        declared
    } else {
        Vec::new()
    };
    for object in &baseline.schema {
        let expected = if object.1 == "sync_events" {
            sync_events
        } else if (matches!(expected_version, 52..=90)
            && collection_artwork_schema::REPLACED
                .iter()
                .any(|(kind, name)| *kind == object.0 && *name == object.1))
            || (expected_version >= 55 && object.0 == "table" && object.1 == "source_resolutions")
            || (expected_version >= 79 && object.0 == "table" && object.1 == "playlist_follows")
            || (expected_version >= 80 && object.0 == "table" && object.1 == "cover_art_lookups")
            || (expected_version >= 82 && object.0 == "table" && object.1 == "sessions")
            || (expected_version >= 86 && object.0 == "table" && object.1 == "users")
            || (expected_version >= 83
                && object.0 == "table"
                && object.1 == "plugin_authorization_attempts")
            || (expected_version >= 67
                && collection_artwork_schema::account_schema::REPLACED
                    .iter()
                    .any(|(kind, name)| *kind == object.0 && *name == object.1))
        {
            artwork
                .iter()
                .find(|current| current.0 == object.0 && current.1 == object.1)
                .unwrap()
        } else {
            object
        };
        assert!(
            after.contains(expected),
            "Migration 0046 changed a prior schema object beyond its exact declaration"
        );
    }
    reference.close().await.unwrap();
    let new_names: Vec<&str> = after
        .iter()
        .filter(|object| !baseline.schema.iter().any(|old| old.1 == object.1))
        .map(|object| object.1.as_str())
        .collect();
    let mut expected = vec![
        "playlist_import_receipt_publication",
        "playlist_follow_receipt_publication",
        "playlist_metadata_refresh_receipts",
        "sqlite_autoindex_playlist_metadata_refresh_receipts_1",
        "playlist_metadata_receipt_immutable",
        "playlist_metadata_receipt_retained",
    ];
    match expected_version {
        46 => {}
        47..=49 | 52..=90 => expected.extend([
            "account_metadata_policies",
            "sqlite_autoindex_account_metadata_policies_1",
            "recording_artist_associations",
            "sqlite_autoindex_recording_artist_associations_1",
            "sqlite_autoindex_recording_artist_associations_2",
            "recording_artist_associations_account",
            "recording_artist_association_scope",
            "recording_artist_association_immutable",
            "recording_artist_association_retained",
        ]),
        _ => panic!("The upgrade fixture requires an exact supported schema endpoint"),
    }
    let declared_audiobooks = if matches!(expected_version, 48 | 49 | 52..=90) {
        let mut reference = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0048_audiobooks.sql"))
            .execute(&mut reference)
            .await
            .unwrap();
        if matches!(expected_version, 49 | 52..=90) {
            sqlx::raw_sql(include_str!(
                "../../migrations/0049_audiobook_continuation.sql"
            ))
            .execute(&mut reference)
            .await
            .unwrap();
        }
        let declared = schema(&mut reference).await;
        reference.close().await.unwrap();
        declared
    } else {
        Vec::new()
    };
    for object in &declared_audiobooks {
        assert!(
            after.contains(object),
            "Each audiobook schema object must match its exact declared migration"
        );
        expected.push(object.1.as_str());
    }
    if matches!(expected_version, 52..=90) {
        for &(kind, name) in collection_artwork_schema::ADDED
            .iter()
            .chain(
                collection_artwork_schema::RANKING_ADDED
                    .iter()
                    .filter(|_| expected_version >= 53),
            )
            .chain(
                collection_artwork_schema::identity_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 54),
            )
            .chain(
                collection_artwork_schema::album_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 56),
            )
            .chain(
                collection_artwork_schema::changes_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 57),
            )
            .chain(
                collection_artwork_schema::pin_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 58),
            )
            .chain(
                collection_artwork_schema::correction_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 59),
            )
            .chain(
                collection_artwork_schema::match_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 60),
            )
            .chain(
                collection_artwork_schema::provider_evidence_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 61),
            )
            .chain(
                collection_artwork_schema::provider_url_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 62),
            )
            .chain(
                collection_artwork_schema::name_match_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 63),
            )
            .chain(
                collection_artwork_schema::tag_identity_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 64),
            )
            .chain(
                collection_artwork_schema::erasure_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 65),
            )
            .chain(
                collection_artwork_schema::automation_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 66),
            )
            .chain(
                collection_artwork_schema::account_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 67),
            )
            .chain(
                collection_artwork_schema::genre_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 68),
            )
            .chain(
                collection_artwork_schema::musicbrainz_genres_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 69),
            )
            .chain(
                collection_artwork_schema::source_unavailability_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 70),
            )
            .chain(
                collection_artwork_schema::credit_matches_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 71),
            )
            .chain(
                collection_artwork_schema::coverage_matches_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 73),
            )
            .chain(
                collection_artwork_schema::review_queue_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 75),
            )
            .chain(
                collection_artwork_schema::recording_id_matches_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 76),
            )
            .chain(
                collection_artwork_schema::recording_groups_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 77),
            )
            .chain(
                collection_artwork_schema::credited_artists_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 78),
            )
            .chain(
                collection_artwork_schema::plugin_connections_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 84),
            )
            .chain(
                collection_artwork_schema::listening_history_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 85),
            )
            .chain(
                collection_artwork_schema::account_avatar_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 87),
            )
            .chain(
                collection_artwork_schema::account_source_preference_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 89),
            )
            .chain(
                collection_artwork_schema::catalog_track_link_schema::ADDED
                    .iter()
                    .filter(|_| expected_version >= 90),
            )
        {
            let object = artwork
                .iter()
                .find(|object| object.0 == kind && object.1 == name)
                .unwrap();
            assert!(
                after.contains(object),
                "each collection artwork addition must match its exact declaration"
            );
            expected.push(name);
        }
        collection_artwork_schema::assert_version(connection, expected_version).await;
    }
    expected.sort_unstable();
    let mut new_names = new_names;
    new_names.sort_unstable();
    assert_eq!(new_names, expected);
    if matches!(expected_version, 48 | 49 | 52..=90) {
        let mut tables = vec!["audiobooks", "audiobook_bookmarks"];
        if matches!(expected_version, 49 | 52..=90) {
            tables.push("audiobook_continuations");
        }
        for table in tables {
            let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {}", quoted(table)))
                .fetch_one(&mut *connection)
                .await
                .unwrap();
            assert_eq!(
                count, 0,
                "Historical playlists must not become books, bookmarks or continuation anchors implicitly"
            );
        }
    }
    for (name, table) in [
        (
            "playlist_import_receipt_publication",
            "playlist_import_receipts",
        ),
        (
            "playlist_follow_receipt_publication",
            "playlist_follow_receipts",
        ),
    ] {
        let index: SchemaObject = (
            "index".into(),
            name.into(),
            table.into(),
            Some(format!(
                "CREATE INDEX {name} ON {table}(publication_sequence)"
            )),
        );
        assert!(
            after.contains(&index),
            "Each new receipt index must cover its exact parent-reference column"
        );
    }
    assert_eq!(version(connection).await, expected_version);
    let migration = sqlx::migrate!("./migrations");
    for version in 46..=expected_version {
        let expected = migration
            .iter()
            .find(|migration| migration.version == version)
            .unwrap();
        let (checksum, success): (Vec<u8>, bool) =
            sqlx::query_as("SELECT checksum,success FROM _sqlx_migrations WHERE version = ?")
                .bind(version)
                .fetch_one(&mut *connection)
                .await
                .unwrap();
        assert!(success && checksum.as_slice() == expected.checksum.as_ref());
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM playlist_metadata_refresh_receipts")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        0
    );
    integrity(connection).await;
}

pub async fn checkpoint_45(connection: &mut SqliteConnection, baseline: &Baseline) -> Baseline {
    assert_eq!(version(connection).await, 45);
    assert!(
        schema(connection).await == baseline.schema,
        "Synthetic history must retain the actual historical schema"
    );
    integrity(connection).await;
    Baseline {
        tables: baseline.tables.clone(),
        rows: rows(connection, &baseline.tables).await,
        schema: baseline.schema.clone(),
    }
}

async fn version(connection: &mut SqliteConnection) -> i64 {
    sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success = 1")
        .fetch_one(connection)
        .await
        .unwrap()
}

const FTS_TABLES: [&str; 6] = [
    "recordings_fts",
    "artists_fts",
    "releases_fts",
    "personal_recordings_fts",
    "account_artists_fts",
    "account_releases_fts",
];

async fn integrity(connection: &mut SqliteConnection) {
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        1
    );
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
    for fts in FTS_TABLES {
        assert!(
            sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM {fts}_docsize"))
                .fetch_one(&mut *connection)
                .await
                .unwrap()
                > 0,
            "The preservation fixture must exercise every populated FTS index"
        );
        sqlx::query(&format!(
            "INSERT INTO {fts}({fts},rank) VALUES ('integrity-check',1)"
        ))
        .execute(&mut *connection)
        .await
        .unwrap();
    }
}

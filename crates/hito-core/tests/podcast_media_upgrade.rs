use hito_core::{Actor, CoreConfig, HostDeviceId, PodcastEpisodeQuery, Services};
use sha2::{Digest, Sha256};
use sqlx::{
    Connection, Row, SqliteConnection, TypeInfo, ValueRef,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use std::{borrow::Cow, collections::BTreeMap};
use uuid::Uuid;

const OWNER: &str = "podcast-migration-owner";
const TOKEN: &str = "owned-podcast-migration-token";
const SUBSCRIPTION: &str = "47000000-0000-4000-8000-000000000001";
const PLAYLIST: &str = "45000000-0000-4000-8000-000000000001";
const OLD_SOURCE: &str = "43000000-0000-4000-8000-000000000001";
const RECORDINGS: [&str; 2] = [
    "42000000-0000-4000-8000-000000000001",
    "42000000-0000-4000-8000-000000000002",
];
const EPISODES: [&str; 2] = [
    "48000000-0000-4000-8000-000000000001",
    "48000000-0000-4000-8000-000000000002",
];
const ENTRIES: [&str; 2] = [
    "46000000-0000-4000-8000-000000000001",
    "46000000-0000-4000-8000-000000000002",
];

#[tokio::test]
async fn upgrade_preserves_private_episodes_and_backfills_stable_sources() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("podcast-upgrade.db");
    let options = SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Delete);
    let all = sqlx::migrate!("./migrations");
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    through(&all, 40).run(&mut connection).await.unwrap();
    sqlx::raw_sql(include_str!("fixtures/podcast_media_upgrade.sql"))
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions(token_hash,user_id,expires_at) VALUES(?,?,4102444800)")
        .bind(format!("{:x}", Sha256::digest(TOKEN.as_bytes())))
        .bind(OWNER)
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO podcast_subscription_receipts(user_id,request_id,request_digest,subscription_id,job_id) VALUES(?,'4a000000-0000-4000-8000-000000000001',?,?,'49000000-0000-4000-8000-000000000001')")
        .bind(OWNER)
        .bind(format!("{:x}", Sha256::digest(b"https://owned-podcast-migration.invalid/feed.xml")))
        .bind(SUBSCRIPTION)
        .execute(&mut connection).await.unwrap();
    assert_version(&mut connection, 40).await;
    assert_fixture(&mut connection).await;
    assert_integrity(&mut connection).await;
    let before = RetainedState::capture(&mut connection).await;
    connection.close().await.unwrap();

    let options = options.create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    assert_version(&mut connection, 40).await;
    before.assert_preserved(&mut connection).await;
    assert_fixture(&mut connection).await;
    through(&all, 41).run(&mut connection).await.unwrap();
    assert_version(&mut connection, 41).await;
    before.assert_preserved(&mut connection).await;
    assert_fixture(&mut connection).await;
    let backfilled = backfilled_sources(&mut connection).await;
    assert_integrity(&mut connection).await;
    connection.close().await.unwrap();

    for _ in 0..2 {
        let services = hito_core::open(CoreConfig::with_shared_directories(
            database.clone(),
            HostDeviceId::new("podcast-migration-host").unwrap(),
            vec![],
        ))
        .await
        .unwrap();
        let actor = services.identity.authenticate(TOKEN).await.unwrap();
        assert_public_state(&services, &actor, &backfilled).await;
        drop(actor);
        drop(services);
    }

    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    assert_version(&mut connection, 41).await;
    before.assert_preserved(&mut connection).await;
    assert_fixture(&mut connection).await;
    assert!(
        backfilled_sources(&mut connection).await == backfilled,
        "reopening core must retain the exact backfilled source identities"
    );
    assert_integrity(&mut connection).await;
    connection.close().await.unwrap();
    assert!(database.is_file());
}

async fn assert_public_state(
    services: &Services,
    actor: &Actor,
    sources: &BTreeMap<String, String>,
) {
    let episodes = services
        .podcasts
        .episodes(actor, SUBSCRIPTION, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(episodes.total, 2);
    assert_eq!(episodes.items.len(), 2);
    for (index, episode) in episodes.items.iter().enumerate() {
        assert_eq!(episode.episode_id.to_string(), EPISODES[index]);
        assert_eq!(episode.recording.id, RECORDINGS[index]);
    }
    let ids = RECORDINGS.map(|id| Uuid::parse_str(id).unwrap());
    let recordings = services
        .catalog
        .recordings_by_ids(actor, &ids)
        .await
        .unwrap();
    assert_eq!(recordings.len(), 2);
    for (index, recording) in recordings.iter().enumerate() {
        let prefix = if index == 0 { "First" } else { "Second" };
        assert_eq!(recording.id, RECORDINGS[index]);
        assert_eq!(recording.title, format!("{prefix} private episode"));
        assert_eq!(recording.artist, format!("{prefix} private author"));
        assert_eq!(
            recording.inherited_title,
            format!("{prefix} inherited episode")
        );
        assert_eq!(recording.inherited_artist, "Owned inherited author");
        assert_eq!(recording.liked, index == 0);
        assert_eq!(
            services
                .collection
                .saved_position(actor, ids[index])
                .await
                .unwrap()
                .unwrap()
                .position_ms,
            if index == 0 { 700 } else { 900 }
        );
        let available = services
            .catalog
            .sources(actor, &recording.id)
            .await
            .unwrap();
        assert_eq!(available.len(), if index == 0 { 2 } else { 1 });
        let podcast = available
            .iter()
            .filter(|source| source.provider == "podcast")
            .collect::<Vec<_>>();
        assert_eq!(podcast.len(), 1);
        let source = podcast[0];
        assert_eq!(sources.get(EPISODES[index]), Some(&source.id));
        assert_eq!(source.recording_id, recording.id);
        assert_eq!(source.label, "Podcast audio");
        assert!(source.device_id.is_none() && source.ready && !source.can_prepare);
        let facts = serde_json::to_string(&available).unwrap();
        assert!(
            !facts.contains("owned-podcast-migration.invalid")
                && !facts.contains("owned-private-canary"),
            "public source facts must omit retained enclosure addresses"
        );
        if index == 0 {
            assert!(available.iter().any(|source| source.id == OLD_SOURCE
                && source.provider == "local"
                && !source.ready));
        }
    }
    let playlist = services.collection.playlist(actor, PLAYLIST).await.unwrap();
    assert_eq!(playlist.entries.len(), 2);
    for (index, entry) in playlist.entries.iter().enumerate() {
        assert_eq!(entry.entry_id, ENTRIES[index]);
        assert_eq!(entry.recording_id.as_deref(), Some(RECORDINGS[0]));
        assert_eq!(entry.position, index as i64);
        assert_eq!(entry.title, "First private episode");
        assert_eq!(entry.artist, "First private author");
        assert!(entry.liked);
    }
}

async fn assert_fixture(connection: &mut SqliteConnection) {
    let retained: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT episode_count,content_revision,
            (SELECT count(*) FROM podcast_episodes),
            (SELECT receipt_count FROM podcast_request_account_counts WHERE user_id=p.user_id),
            (SELECT receipt_count FROM podcast_request_total WHERE singleton=1)
         FROM podcast_subscriptions p WHERE id=?",
    )
    .bind(SUBSCRIPTION)
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(retained, (2, 7, 2, 1, 1));
    let sort_keys: Vec<(String, i64)> =
        sqlx::query_as("SELECT id,sort_key FROM podcast_episodes ORDER BY id")
            .fetch_all(connection)
            .await
            .unwrap();
    assert_eq!(
        sort_keys,
        vec![
            (EPISODES[0].into(), 1700000000),
            (EPISODES[1].into(), i64::MIN)
        ]
    );
}

#[derive(sqlx::FromRow)]
struct BackfilledSource {
    episode_id: String,
    source_id: String,
    episode_recording_id: String,
    source_recording_id: String,
    provider_id: String,
    device_id: Option<String>,
    ready: i64,
}

async fn backfilled_sources(connection: &mut SqliteConnection) -> BTreeMap<String, String> {
    let rows: Vec<BackfilledSource> = sqlx::query_as(
        "SELECT mapping.episode_id,s.id AS source_id,e.recording_id AS episode_recording_id,
            s.recording_id AS source_recording_id,s.provider_id,s.device_id,s.ready
         FROM podcast_sources mapping JOIN podcast_episodes e ON e.id=mapping.episode_id
         JOIN sources s ON s.id=mapping.source_id WHERE s.provider='podcast' ORDER BY e.id",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    let mut identities = BTreeMap::new();
    for (index, source) in rows.into_iter().enumerate() {
        assert_eq!(source.episode_id, EPISODES[index]);
        assert_eq!(source.episode_recording_id, RECORDINGS[index]);
        assert_eq!(source.source_recording_id, source.episode_recording_id);
        assert_eq!(source.provider_id, source.episode_id);
        assert!(source.device_id.is_none() && source.ready == 1);
        let id = Uuid::parse_str(&source.source_id).unwrap();
        assert!(id.get_version_num() == 4 && id.get_variant() == uuid::Variant::RFC4122);
        assert_eq!(id.to_string(), source.source_id);
        identities.insert(source.episode_id, source.source_id);
    }
    assert_eq!(
        identities
            .values()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    let totals: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM sources),(SELECT count(*) FROM podcast_sources),(SELECT count(*) FROM current_podcast_sources)")
        .fetch_one(connection).await.unwrap();
    assert_eq!(totals, (3, 2, 2));
    identities
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

#[derive(PartialEq, Eq)]
enum Cell {
    Null,
    Integer(i64),
    Real(u64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

struct RetainedState {
    schema: BTreeMap<String, String>,
    tables: Vec<(String, Vec<Vec<Cell>>)>,
}

impl RetainedState {
    async fn capture(connection: &mut SqliteConnection) -> Self {
        let tables: Vec<(String, i64)> = sqlx::query_as(
            "SELECT schema.name,metadata.wr FROM sqlite_schema schema
             JOIN pragma_table_list metadata ON metadata.schema='main' AND metadata.name=schema.name
             WHERE schema.type='table' ORDER BY schema.name",
        )
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        let mut retained = Vec::new();
        for (table, without_rowid) in tables {
            let columns: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM pragma_table_xinfo(?) WHERE hidden!=1 ORDER BY cid",
            )
            .bind(&table)
            .fetch_all(&mut *connection)
            .await
            .unwrap();
            if table == "podcast_episodes" {
                assert!(columns.iter().any(|column| column == "sort_key"));
            }
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
            let filter = match table.as_str() {
                "_sqlx_migrations" => " WHERE version<=40",
                "sources" => " WHERE provider!='podcast'",
                _ => "",
            };
            let statement = format!(
                "SELECT {selected} FROM {}{filter} ORDER BY {order}",
                quoted(&table)
            );
            let rows = typed_rows(connection, &statement).await;
            retained.push((statement, rows));
        }
        Self {
            schema: schema(connection).await,
            tables: retained,
        }
    }

    async fn assert_preserved(&self, connection: &mut SqliteConnection) {
        let after = schema(connection).await;
        assert!(
            self.schema
                .iter()
                .all(|(name, definition)| after.get(name) == Some(definition)),
            "the media migration must retain every original schema definition"
        );
        for (statement, before) in &self.tables {
            assert!(
                typed_rows(connection, statement).await == *before,
                "every original typed value, generated column, rowid and migration checksum must survive"
            );
        }
    }
}

async fn schema(connection: &mut SqliteConnection) -> BTreeMap<String, String> {
    sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE type IN ('table','trigger','view','index') AND sql IS NOT NULL ORDER BY name")
        .fetch_all(connection).await.unwrap().into_iter().collect()
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
                        _ => panic!("the owned migration fixture has an unsupported SQLite type"),
                    }
                })
                .collect()
        })
        .collect()
}

async fn assert_integrity(connection: &mut SqliteConnection) {
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty()
    );
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' AND sql LIKE '%USING fts5%' ORDER BY name")
        .fetch_all(&mut *connection).await.unwrap();
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

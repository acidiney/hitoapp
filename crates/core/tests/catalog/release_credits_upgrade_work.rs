#[path = "../fixtures/release_credits_upgrade.rs"]
mod fixture;
use hito_core::test_support::support;

use futures_util::TryStreamExt;
use hito_core::Browse;
use sha2::{Digest, Sha256};
use sqlx::{
    Connection, SqliteConnection,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const SMALL: i64 = 128;
const LARGE: i64 = 2048;
const ACCOUNTS: i64 = 10;
const FIXED_WORK: u64 = 200_000;
const WORK_PER_ALBUM: u64 = 20_000;
const OTHER_TOKEN: &str = "release-credit-work-other-token";
type Counts = (i64, i64, i64, i64, i64, i64, i64);
type Snapshot = Vec<(&'static str, u64, String)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Statistics {
    Absent,
    BeforeGrowth,
    AfterGrowth,
}

#[tokio::test]
async fn rebuilding_releases_keeps_foreign_key_work_bounded_with_ten_account_projections() {
    for statistics in [
        Statistics::Absent,
        Statistics::BeforeGrowth,
        Statistics::AfterGrowth,
    ] {
        let small = migrate(SMALL, statistics).await;
        let large = migrate(LARGE, statistics).await;
        assert!(
            large <= small * 24 + FIXED_WORK,
            "production release upgrade exceeded the index-growth allowance with {statistics:?}: small={small}, large={large}"
        );
    }
}

async fn migrate(count: i64, statistics: Statistics) -> u64 {
    let legacy = fixture::seed("http://127.0.0.1:9/").await;
    let options = SqliteConnectOptions::new()
        .filename(legacy.directory.path().join("hito.db"))
        .create_if_missing(false)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Delete);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let baseline = counts(&mut connection).await;
    seed_accounts(&mut connection).await;
    if statistics == Statistics::BeforeGrowth {
        albums(&mut connection, 1, 32).await;
        sqlx::query("ANALYZE")
            .execute(&mut connection)
            .await
            .unwrap();
        albums(&mut connection, 33, count).await;
    } else {
        albums(&mut connection, 1, count).await;
        if statistics == Statistics::AfterGrowth {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
    }
    assert_fixture(&mut connection, baseline, count).await;
    assert_statistics(&mut connection, baseline.3, count, statistics).await;
    let before = snapshot(&mut connection).await;
    connection.close().await.unwrap();

    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    assert_fixture(&mut connection, baseline, count).await;
    assert_statistics(&mut connection, baseline.3, count, statistics).await;
    assert_eq!(version(&mut connection).await, 44);
    assert_eq!(snapshot(&mut connection).await, before);
    let budget = FIXED_WORK + count as u64 * WORK_PER_ALBUM;
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
    let all = sqlx::migrate!("./migrations");
    let measured = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 45)
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
        assert_eq!(
            version(&mut connection).await,
            45,
            "the measured operation must run actual production migration0045"
        );
        assert_fixture(&mut connection, baseline, count).await;
        assert_eq!(snapshot(&mut connection).await, before);
        assert_integrity(&mut connection).await;
    }
    connection.close().await.unwrap();
    eprintln!(
        "Release credit upgrade statistics={statistics:?} added_albums={count} accounts={ACCOUNTS} VM_steps~{work} budget={budget} progress_aborted={aborted}"
    );
    assert!(
        !aborted,
        "production migration0045 exceeded its linear foreign-key work allowance: added_albums={count}, accounts={ACCOUNTS}, statistics={statistics:?}, steps~{work}, budget={budget}"
    );
    assert!(
        result.is_ok(),
        "production migration failed before its VM budget; this is not the expected performance regression"
    );
    assert!(work > 0 && work < budget);
    assert_public_output(&legacy, count).await;
    work
}

async fn seed_accounts(connection: &mut SqliteConnection) {
    sqlx::raw_sql(
        "CREATE TEMP TABLE hito_release_credit_work_accounts (
            position INTEGER PRIMARY KEY, user_id TEXT NOT NULL,
            artist_id TEXT NOT NULL, credit TEXT NOT NULL);
         INSERT INTO hito_release_credit_work_accounts VALUES
            (0,'legacy-admin','release-work-private-0','Private scale credit 0');
         WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<9)
         INSERT INTO hito_release_credit_work_accounts
            SELECT i,'release-work-user-'||i,'release-work-private-'||i,'Private scale credit '||i FROM n;
         INSERT INTO users(id,username,password_hash)
            SELECT user_id,user_id,'unused-owned-fixture-hash'
            FROM hito_release_credit_work_accounts WHERE position>0;
         INSERT INTO libraries(id,name) VALUES ('release-work-library','Owned shared work library');
         INSERT INTO library_members(library_id,user_id)
            SELECT 'release-work-library',user_id FROM hito_release_credit_work_accounts;
         INSERT INTO account_artists(user_id,id,library_id,name)
            SELECT user_id,artist_id,'release-work-library',credit FROM hito_release_credit_work_accounts;
         CREATE TEMP TABLE hito_release_credit_work_numbers(i INTEGER PRIMARY KEY);",
    )
    .execute(&mut *connection)
    .await
    .unwrap();
    sqlx::query("INSERT INTO sessions(token_hash,user_id,expires_at) VALUES (?,'release-work-user-1',4102444800)")
        .bind(format!("{:x}", Sha256::digest(OTHER_TOKEN.as_bytes())))
        .execute(&mut *connection)
        .await
        .unwrap();
}

async fn albums(connection: &mut SqliteConnection, first: i64, last: i64) {
    let mut transaction = connection.begin().await.unwrap();
    sqlx::query("DELETE FROM hito_release_credit_work_numbers")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?) INSERT INTO hito_release_credit_work_numbers SELECT i FROM n")
        .bind(first).bind(last).execute(&mut *transaction).await.unwrap();
    sqlx::raw_sql(include_str!("../fixtures/release_credits_upgrade_work.sql"))
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn counts(connection: &mut SqliteConnection) -> Counts {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM releases), (SELECT count(*) FROM recordings),
            (SELECT count(*) FROM sources), (SELECT count(*) FROM source_metadata),
            (SELECT count(*) FROM account_releases), (SELECT count(*) FROM account_source_credits),
            (SELECT count(*) FROM personal_release_metadata)",
    )
    .fetch_one(connection)
    .await
    .unwrap()
}

async fn assert_fixture(connection: &mut SqliteConnection, baseline: Counts, count: i64) {
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(enabled, 1, "the actual migration must enforce foreign keys");
    assert_eq!(
        counts(connection).await,
        (
            baseline.0 + count,
            baseline.1 + count,
            baseline.2 + count,
            baseline.3 + count,
            baseline.4 + count * ACCOUNTS,
            baseline.5 + count * ACCOUNTS,
            baseline.6 + count * ACCOUNTS,
        ),
        "every rebuilt-parent child must contain representative growing rows"
    );
    let authority: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users),
            (SELECT count(*) FROM library_members WHERE library_id='release-work-library'),
            (SELECT count(*) FROM library_access_grants WHERE library_id='release-work-library')",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(authority, (ACCOUNTS, ACCOUNTS, ACCOUNTS));
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn assert_statistics(
    connection: &mut SqliteConnection,
    baseline_sources: i64,
    count: i64,
    statistics: Statistics,
) {
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE name IN ('sqlite_stat1','sqlite_stat4')",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    if statistics == Statistics::Absent {
        assert_eq!(
            tables, 0,
            "the absent case must have no STAT1 or STAT4 history"
        );
        return;
    }
    assert!(tables > 0);
    let stat: String =
        sqlx::query_scalar("SELECT stat FROM sqlite_stat1 WHERE idx='source_metadata_release'")
            .fetch_one(connection)
            .await
            .unwrap();
    let analyzed_rows: i64 = stat.split_whitespace().next().unwrap().parse().unwrap();
    let analyzed_albums = match statistics {
        Statistics::BeforeGrowth => 32,
        Statistics::AfterGrowth => count,
        Statistics::Absent => unreachable!(),
    };
    assert_eq!(analyzed_rows, baseline_sources + analyzed_albums);
}

async fn snapshot(connection: &mut SqliteConnection) -> Snapshot {
    let mut snapshots = Vec::new();
    for table in [
        "users",
        "sessions",
        "libraries",
        "library_members",
        "library_access_grants",
        "releases",
        "account_releases",
        "artists",
        "account_artists",
        "recordings",
        "sources",
        "local_files",
        "source_metadata",
        "source_artist_credits",
        "account_source_credits",
        "account_recording_credits",
        "metadata_observations",
        "provider_items",
        "provider_sources",
        "provider_source_state",
        "personal_recording_metadata",
        "personal_release_metadata",
        "likes",
        "listening_progress",
        "playlists",
        "playlist_entries",
        "playlist_imports",
        "playlist_import_entries",
        "playlist_import_receipts",
        "sync_events",
        "sqlite_sequence",
        "_sqlx_migrations",
    ] {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_xinfo(?) ORDER BY cid")
                .bind(table)
                .fetch_all(&mut *connection)
                .await
                .unwrap();
        let fields = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect::<Vec<_>>()
            .join(",");
        let filter = if table == "_sqlx_migrations" {
            " WHERE version<=44"
        } else {
            ""
        };
        let query =
            format!("SELECT json_array(quote(rowid),{fields}) FROM {table}{filter} ORDER BY rowid");
        let mut rows = sqlx::query_scalar::<_, String>(&query).fetch(&mut *connection);
        let mut digest = Sha256::new();
        let mut count = 0;
        while let Some(row) = rows.try_next().await.unwrap() {
            digest.update((row.len() as u64).to_le_bytes());
            digest.update(row.as_bytes());
            count += 1;
        }
        snapshots.push((table, count, format!("{:x}", digest.finalize())));
    }
    snapshots
}

async fn version(connection: &mut SqliteConnection) -> i64 {
    sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
        .fetch_one(connection)
        .await
        .unwrap()
}

async fn assert_integrity(connection: &mut SqliteConnection) {
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");
    for index in [
        "releases_fts",
        "account_releases_fts",
        "artists_fts",
        "account_artists_fts",
        "recordings_fts",
        "personal_recordings_fts",
    ] {
        sqlx::query(&format!(
            "INSERT INTO {index}({index},rank) VALUES ('integrity-check',1)"
        ))
        .execute(&mut *connection)
        .await
        .unwrap();
    }
}

async fn assert_public_output(legacy: &fixture::Legacy, count: i64) {
    let (hito, actor) = legacy.open().await;
    let albums = hito
        .catalog
        .releases(
            &actor,
            Browse {
                q: Some("Private scale album".into()),
                limit: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(albums.total, count);
    assert_eq!(albums.items.len(), 2);
    assert_eq!(albums.items[0].id, "release-work-album-1");
    assert_eq!(albums.items[1].id, "release-work-album-2");
    for album in &albums.items {
        assert_eq!(album.artist.as_deref(), Some("Private scale credit 0"));
        assert_eq!(album.track_count, 1);
        assert_eq!(album.duration_ms, 12_000);
    }
    let release = hito
        .catalog
        .release(&actor, "release-work-album-1", Browse::default())
        .await
        .unwrap();
    assert_eq!(release.tracks.total, 1);
    assert_eq!(release.tracks.items[0].entry_id, "release-work-source-1");
    assert_eq!(
        release.tracks.items[0].recording.id,
        "release-work-recording-1"
    );
    assert!(!release.tracks.items[0].recording.available);
    let metadata = hito
        .catalog
        .release_metadata(&actor, "release-work-album-1", Default::default())
        .await
        .unwrap();
    assert_eq!(
        metadata.artist.value.as_deref(),
        Some("Private scale credit 0")
    );
    assert_eq!(
        metadata.artist.inherited.as_deref(),
        Some("Owned scale album credit")
    );
    let other = hito.identity.authenticate(OTHER_TOKEN).await.unwrap();
    let other_release = hito
        .catalog
        .release(&other, "release-work-album-1", Browse::default())
        .await
        .unwrap();
    assert_eq!(
        other_release.release.artist.as_deref(),
        Some("Private scale credit 1")
    );
    assert_eq!(other_release.release.id, release.release.id);
    assert_ne!(other_release.release.artist_id, release.release.artist_id);
    assert_eq!(
        support::audio_bytes(
            hito.playback
                .audio_file(&actor, fixture::SOURCE)
                .await
                .unwrap()
        ),
        std::fs::read(&legacy.original).unwrap()
    );
    let playlist = hito
        .collection
        .playlist(&actor, fixture::PLAYLIST)
        .await
        .unwrap();
    assert_eq!(
        playlist
            .entries
            .iter()
            .map(|entry| (entry.position, entry.recording_id.as_deref()))
            .collect::<Vec<_>>(),
        [
            (0, Some(fixture::RECORDING)),
            (1, Some(fixture::PROVIDER_RECORDING))
        ]
    );
}

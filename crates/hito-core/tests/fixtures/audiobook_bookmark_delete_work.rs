use futures_util::TryStreamExt;
#[path = "collection_artwork_schema.rs"]
mod collection_artwork_schema;
use sha2::{Digest, Sha256};
use sqlx::{
    Connection, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};

pub const FOLLOW: &str = "work-follow";
pub const BOOKS: i64 = 400;
pub const MARKERS: i64 = 100_000;
pub const RETIRED_CHAPTERS: u64 = 20;
pub type Snapshot = Vec<(&'static str, u64, String)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Statistics {
    Absent,
    Inherited,
    Fresh,
}

pub struct Database {
    pub directory: tempfile::TempDir,
    pub options: SqliteConnectOptions,
}

pub async fn seed(statistics: Statistics) -> Database {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("audiobook-work.db"))
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Delete);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::migrate!("./migrations")
        .run(&mut connection)
        .await
        .unwrap();
    let latest: i64 =
        sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        latest, 54,
        "The workload must use the full actual production migration history"
    );
    collection_artwork_schema::assert_current(&mut connection).await;
    let mut tx = connection.begin().await.unwrap();
    sqlx::raw_sql(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<9)
         INSERT INTO users(id,username,password_hash)
         SELECT 'work-user-'||i,'work-user-'||i,'unused-owned-fixture-hash' FROM n;
         INSERT INTO libraries(id,name) VALUES ('work-library','Owned work library');
         INSERT INTO library_members(library_id,user_id) SELECT 'work-library',id FROM users;
         INSERT INTO recordings(id,library_id,title,artist,duration_ms)
         VALUES ('work-recording','work-library','Owned work recording','Owned work performer',300000);
         INSERT INTO plugin_services(id,name,base_url)
         VALUES ('work-service','Owned work provider','https://provider.invalid/');
         INSERT INTO plugin_installations(id,user_id,service_id,manifest)
         VALUES ('work-installation','work-user-0','work-service',
            '{\"id\":\"org.hito.work\",\"name\":\"Owned work provider\",\"protocol_version\":1,\"capabilities\":[\"playlists\"]}');
         INSERT INTO playlists(id,user_id,name,kind)
         VALUES ('work-follow','work-user-0','Owned refreshed playlist','followed');
         INSERT INTO playlist_follows(playlist_id,user_id,installation_id,installation_revision,
            service_id,service_revision,snapshot_installation_revision,snapshot_service_revision,
            provider,provider_playlist_id,snapshot,content_digest,attribution,interval_seconds,next_refresh_at)
         VALUES ('work-follow','work-user-0','work-installation',1,'work-service',1,1,1,
            'org.hito.work','owned-list','owned-snapshot',printf('%064d',0),
            '{\"label\":\"Owned work provider\"}',3600,3600);
         WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<19)
         INSERT INTO playlist_entries(playlist_id,id,recording_id,position,deleted)
         SELECT 'work-follow','work-retired-'||i,'work-recording',i,1 FROM n;
         CREATE TEMP TABLE hito_audiobook_work_numbers(i INTEGER PRIMARY KEY);"
    ).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    books(&mut connection, 0, 9).await;
    if statistics == Statistics::Inherited {
        sqlx::query("ANALYZE")
            .execute(&mut connection)
            .await
            .unwrap();
    }
    books(&mut connection, 10, BOOKS - 1).await;
    if statistics == Statistics::Fresh {
        sqlx::query("ANALYZE")
            .execute(&mut connection)
            .await
            .unwrap();
    }
    assert_fixture(&mut connection).await;
    assert_statistics(&mut connection, statistics).await;
    connection.close().await.unwrap();
    Database {
        directory,
        options: options.create_if_missing(false),
    }
}

async fn books(connection: &mut SqliteConnection, first: i64, last: i64) {
    let mut tx = connection.begin().await.unwrap();
    sqlx::query("DELETE FROM hito_audiobook_work_numbers")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO hito_audiobook_work_numbers SELECT i FROM n",
    )
    .bind(first)
    .bind(last)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("audiobook_bookmark_delete_work.sql"))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

pub async fn assert_fixture(connection: &mut SqliteConnection) {
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(
        enabled, 1,
        "The actual parent delete must enforce its full foreign-key graph"
    );
    let counts: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users), (SELECT count(*) FROM audiobooks),
            (SELECT count(*) FROM audiobook_bookmarks),
            (SELECT count(*) FROM audiobook_bookmarks WHERE deleted=0),
            (SELECT count(*) FROM audiobook_bookmarks WHERE deleted=1),
            (SELECT count(*) FROM current_playlist_follow_authority WHERE playlist_id='work-follow')"
    ).fetch_one(&mut *connection).await.unwrap();
    assert_eq!(counts, (10, BOOKS, MARKERS, MARKERS / 2, MARKERS / 2, 1));
    let accounts: Vec<(i64, i64)> =
        sqlx::query_as("SELECT count(*),sum(deleted=0) FROM audiobook_bookmarks GROUP BY user_id")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    assert!(
        accounts.len() == 10 && accounts.iter().all(|counts| *counts == (10_000, 5_000)),
        "The retained work must be distributed across ten actual account namespaces"
    );
    let books: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT count(*),sum(deleted=0) FROM audiobook_bookmarks GROUP BY user_id,audiobook_id",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    assert!(
        books.len() == BOOKS as usize && books.iter().all(|counts| *counts == (250, 125)),
        "Every owned book must remain below the public 256-active-bookmark limit"
    );
    let anchored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audiobook_bookmarks marker
         JOIN audiobooks book ON book.playlist_id=marker.audiobook_id AND book.user_id=marker.user_id
         JOIN playlist_entries chapter ON chapter.playlist_id=book.playlist_id AND chapter.id=marker.entry_id
         JOIN recordings recording ON recording.id=marker.recording_id
         JOIN library_members access ON access.library_id=recording.library_id AND access.user_id=marker.user_id
         WHERE chapter.recording_id=marker.recording_id AND chapter.deleted=0"
    ).fetch_one(&mut *connection).await.unwrap();
    assert_eq!(
        anchored, MARKERS,
        "Every retained marker must have a real accessible chapter anchor"
    );
    let references: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audiobook_bookmarks WHERE audiobook_id=?")
            .bind(FOLLOW)
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(
        references, 0,
        "Follow refresh must delete unrelated entries, not referenced book chapters"
    );
    let retired: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM playlist_entries WHERE playlist_id=? AND deleted=1",
    )
    .bind(FOLLOW)
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(retired, RETIRED_CHAPTERS as i64);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty(),
        "The full fixture must retain valid production foreign keys"
    );
}

pub async fn assert_statistics(connection: &mut SqliteConnection, statistics: Statistics) {
    let tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE name IN ('sqlite_stat1','sqlite_stat4')",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    if statistics == Statistics::Absent {
        assert_eq!(
            tables, 0,
            "The absent-statistics probe must not inherit ANALYZE history"
        );
    } else {
        assert!(tables > 0);
        let stat: String = sqlx::query_scalar(
            "SELECT stat FROM sqlite_stat1 WHERE idx='audiobook_bookmarks_account'",
        )
        .fetch_one(connection)
        .await
        .unwrap();
        let rows: i64 = stat.split_whitespace().next().unwrap().parse().unwrap();
        assert_eq!(
            rows,
            if statistics == Statistics::Inherited {
                2_500
            } else {
                MARKERS
            },
            "The reopened probe must retain genuinely inherited or fresh marker cardinality statistics"
        );
    }
}

pub async fn snapshot(connection: &mut SqliteConnection) -> Snapshot {
    let mut snapshots = Vec::new();
    for table in [
        "users",
        "libraries",
        "library_members",
        "library_access_grants",
        "recordings",
        "artists",
        "plugin_services",
        "plugin_installations",
        "playlists",
        "playlist_follows",
        "playlist_entries",
        "audiobooks",
        "audiobook_bookmarks",
        "audiobook_continuations",
        "_sqlx_migrations",
        "sqlite_sequence",
    ] {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_xinfo(?) ORDER BY cid")
                .bind(table)
                .fetch_all(&mut *connection)
                .await
                .unwrap();
        let fields = columns
            .iter()
            .map(|column| format!("quote(\"{}\")", column.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!("SELECT json_array(quote(rowid),{fields}) FROM {table} ORDER BY rowid");
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

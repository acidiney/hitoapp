#![allow(dead_code)]

use sqlx::{Connection, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;

pub const REPLACED: [(&str, &str); 1] = [("table", "sync_events")];

pub const ADDED_DECLARED: [(&str, &str); 5] = [
    ("table", "playlist_metadata_refresh_receipts"),
    ("trigger", "playlist_metadata_receipt_immutable"),
    ("trigger", "playlist_metadata_receipt_retained"),
    ("index", "playlist_import_receipt_publication"),
    ("index", "playlist_follow_receipt_publication"),
];

pub const ADDED_IMPLICIT: [(&str, &str); 1] = [(
    "index",
    "sqlite_autoindex_playlist_metadata_refresh_receipts_1",
)];

pub const ADDED_INDEXES: [(&str, &str); 3] = [
    (
        "playlist_import_receipts",
        "playlist_import_receipt_publication",
    ),
    (
        "playlist_follow_receipts",
        "playlist_follow_receipt_publication",
    ),
    (
        "playlist_metadata_refresh_receipts",
        "sqlite_autoindex_playlist_metadata_refresh_receipts_1",
    ),
];

pub async fn reference() -> SqliteConnection {
    let mut reference = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 46)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&mut reference)
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&mut reference)
            .await
            .unwrap(),
        46
    );
    reference
}

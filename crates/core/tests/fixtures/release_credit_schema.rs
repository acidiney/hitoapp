#![allow(dead_code)]

use sqlx::{Connection, SqliteConnection};

#[derive(Clone, Copy)]
pub enum ObjectKind {
    Table,
    Trigger,
    View,
    Index,
}

impl ObjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Trigger => "trigger",
            Self::View => "view",
            Self::Index => "index",
        }
    }
}

pub const REPLACED: [(ObjectKind, &str); 6] = [
    (ObjectKind::Table, "releases"),
    (ObjectKind::Table, "account_releases"),
    (ObjectKind::Trigger, "release_artist_insert"),
    (ObjectKind::Trigger, "release_artist_update"),
    (ObjectKind::View, "artist_releases"),
    (ObjectKind::View, "account_artist_releases"),
];

pub const ADDED: [(ObjectKind, &str); 5] = [
    (ObjectKind::Index, "recordings_release"),
    (ObjectKind::Index, "account_releases_release"),
    (ObjectKind::Index, "account_source_releases"),
    (ObjectKind::Index, "personal_release_lookup"),
    (ObjectKind::View, "provider_release_appearances"),
];

pub const ADDED_INDEXES: [(&str, &str); 4] = [
    ("recordings", "recordings_release"),
    ("account_releases", "account_releases_release"),
    ("account_source_credits", "account_source_releases"),
    ("personal_release_metadata", "personal_release_lookup"),
];

pub async fn reference(baseline: &mut SqliteConnection) -> SqliteConnection {
    let migration = include_str!("../../migrations/0045_optional_release_credits.sql");
    let mut reference = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    for name in [
        "recordings",
        "account_source_credits",
        "personal_release_metadata",
    ] {
        let definition: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE type='table' AND name=?")
                .bind(name)
                .fetch_one(&mut *baseline)
                .await
                .unwrap();
        sqlx::raw_sql(&definition)
            .execute(&mut reference)
            .await
            .unwrap();
    }
    for name in ["releases", "account_releases"] {
        let temporary = format!("{name}_next");
        sqlx::raw_sql(&statement(migration, "TABLE", &temporary, ";"))
            .execute(&mut reference)
            .await
            .unwrap();
        sqlx::query(&format!("ALTER TABLE {temporary} RENAME TO {name}"))
            .execute(&mut reference)
            .await
            .unwrap();
    }
    for (_, name) in ADDED
        .iter()
        .filter(|(kind, _)| matches!(kind, ObjectKind::Index))
    {
        sqlx::raw_sql(&statement(migration, "INDEX", name, ";"))
            .execute(&mut reference)
            .await
            .unwrap();
    }
    for (_, name) in REPLACED
        .iter()
        .filter(|(kind, _)| matches!(kind, ObjectKind::Trigger))
    {
        sqlx::raw_sql(&statement(migration, "TRIGGER", name, "END;"))
            .execute(&mut reference)
            .await
            .unwrap();
    }
    for (_, name) in REPLACED
        .iter()
        .chain(ADDED.iter())
        .filter(|(kind, _)| matches!(kind, ObjectKind::View))
    {
        sqlx::raw_sql(&statement(migration, "VIEW", name, ";"))
            .execute(&mut reference)
            .await
            .unwrap();
    }
    reference
}

fn statement(migration: &str, kind: &str, name: &str, terminator: &str) -> String {
    let prefix = format!("CREATE {kind} {name} ");
    let definition = migration
        .split(&prefix)
        .nth(1)
        .unwrap_or_else(|| panic!("the release credit migration must declare {kind} {name}"))
        .split(terminator)
        .next()
        .unwrap();
    format!("{prefix}{definition}{terminator}")
}

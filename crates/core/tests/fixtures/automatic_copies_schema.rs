use sqlx::SqliteConnection;

const MIGRATION: &str = include_str!("../../migrations/0105_automatic_copies.sql");

pub const ADDED: &[(&str, &str)] = &[
    ("table", "account_automatic_copies"),
    ("index", "sqlite_autoindex_account_automatic_copies_1"),
    ("table", "automatic_copies"),
    ("index", "sqlite_autoindex_automatic_copies_1"),
    ("table", "upcoming_recordings"),
    ("index", "sqlite_autoindex_upcoming_recordings_1"),
    ("index", "upcoming_recordings_due"),
    ("index", "upcoming_recordings_account"),
    ("view", "chosen_download_intents"),
];

pub const REPLACED: &[(&str, &str)] = &[
    ("table", "download_jobs"),
    ("table", "listening_history"),
    ("view", "retained_download_intents"),
];

fn listed(list: &[(&str, &str)], object: &super::SchemaObject) -> bool {
    list.iter()
        .any(|(kind, name)| object.0 == *kind && object.1 == *name)
}

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    assert!(!before.iter().any(|object| listed(ADDED, object)));
    sqlx::raw_sql(MIGRATION)
        .execute(&mut connection)
        .await
        .unwrap();
    let after = super::schema(&mut connection).await;
    assert_eq!(
        after.iter().filter(|object| listed(ADDED, object)).count(),
        ADDED.len()
    );
    let mut replaced = Vec::new();
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0105 must not remove prior schema objects");
        if current != object {
            replaced.push((object.0.as_str(), object.1.as_str()));
            assert!(
                changed_as_declared(
                    &object.0,
                    &object.1,
                    object.3.as_deref().unwrap_or_default(),
                    current.3.as_deref().unwrap_or_default(),
                ),
                "0105 must change {} {} exactly as declared",
                object.0,
                object.1
            );
        }
    }
    replaced.sort();
    assert_eq!(replaced, REPLACED.to_vec());
    assert_eq!(after.len(), before.len() + ADDED.len());
    connection
}

pub fn changed_as_declared(kind: &str, name: &str, before: &str, after: &str) -> bool {
    match (kind, name) {
        ("view", "retained_download_intents") => {
            let start = MIGRATION
                .find("CREATE VIEW retained_download_intents AS")
                .expect("0105 must declare the retained download view");
            let end = MIGRATION[start..].find(';').unwrap() + start;
            after == &MIGRATION[start..end]
        }
        ("table", "download_jobs") => with_column(
            before,
            after,
            "busy_retries INTEGER NOT NULL DEFAULT 0 CHECK(busy_retries >= 0)",
        ),
        ("table", "listening_history") => with_column(
            before,
            after,
            "play_count INTEGER NOT NULL DEFAULT 0 CHECK(play_count >= 0)",
        ),
        _ => false,
    }
}

fn with_column(before: &str, after: &str, column: &str) -> bool {
    let added = format!(", {column}");
    after.matches(&added).count() == 1 && after.replacen(&added, "", 1) == before
}

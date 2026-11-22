use sqlx::SqliteConnection;

pub const ADDED: &[(&str, &str)] = &[
    ("table", "playing_recordings"),
    ("index", "sqlite_autoindex_playing_recordings_1"),
    ("index", "playing_recordings_account"),
    ("table", "played_copy_requests"),
    ("index", "sqlite_autoindex_played_copy_requests_1"),
    ("index", "played_copy_requests_recent"),
];

fn added(object: &super::SchemaObject) -> bool {
    ADDED
        .iter()
        .any(|(kind, name)| object.0 == *kind && object.1 == *name)
}

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    assert!(!before.iter().any(added));
    sqlx::raw_sql(include_str!(
        "../../migrations/0106_automatic_copy_plays.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    let after = super::schema(&mut connection).await;
    assert_eq!(
        after.iter().filter(|object| added(object)).count(),
        ADDED.len()
    );
    assert_eq!(
        after
            .into_iter()
            .filter(|object| !added(object))
            .collect::<Vec<_>>(),
        before
    );
    connection
}

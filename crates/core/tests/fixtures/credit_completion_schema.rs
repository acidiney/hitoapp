use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["artist_association_completion_matches"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "artist_association_completion_matches"),
    (
        "index",
        "sqlite_autoindex_artist_association_completion_matches_1",
    ),
    ("trigger", "artist_association_completion_match_immutable"),
    ("trigger", "artist_association_completion_match_retained"),
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
        "../../migrations/0102_credit_completion_matches.sql"
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

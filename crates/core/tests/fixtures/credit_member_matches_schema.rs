use sqlx::SqliteConnection;

pub const ADDED: &[(&str, &str)] = &[
    ("table", "artist_association_member_matches"),
    (
        "index",
        "sqlite_autoindex_artist_association_member_matches_1",
    ),
    ("trigger", "artist_association_member_match_immutable"),
    ("trigger", "artist_association_member_match_retained"),
    ("table", "artist_name_misses"),
    ("index", "sqlite_autoindex_artist_name_misses_1"),
    ("table", "account_credit_splits"),
    ("index", "sqlite_autoindex_account_credit_splits_1"),
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
        "../../migrations/0103_credit_member_matches.sql"
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

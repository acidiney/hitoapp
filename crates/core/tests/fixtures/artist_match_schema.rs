use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["artist_association_matches", "artist_match_dismissals"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "artist_association_matches"),
    ("index", "sqlite_autoindex_artist_association_matches_1"),
    ("trigger", "artist_association_match_immutable"),
    ("trigger", "artist_association_match_retained"),
    ("table", "artist_match_dismissals"),
    ("index", "sqlite_autoindex_artist_match_dismissals_1"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(60).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0060 must retain every exact 0059 schema object without replacements"
        );
    }
    let mut added = after
        .iter()
        .filter(|object| {
            !before
                .iter()
                .any(|old| old.0 == object.0 && old.1 == object.1)
        })
        .map(|object| (object.0.as_str(), object.1.as_str()))
        .collect::<Vec<_>>();
    let mut expected = ADDED.to_vec();
    added.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        added, expected,
        "Every additive 0060 artist matching object must be named exactly"
    );
    connection
}

pub async fn assert_empty(connection: &mut SqliteConnection) {
    for table in TABLES {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(
            count, 0,
            "Historical accounts must not imply an automatic match or a dismissed suggestion"
        );
    }
}

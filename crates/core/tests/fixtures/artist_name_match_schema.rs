use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["artist_association_name_matches", "artist_name_checks"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "artist_association_name_matches"),
    (
        "index",
        "sqlite_autoindex_artist_association_name_matches_1",
    ),
    ("trigger", "artist_association_name_match_immutable"),
    ("trigger", "artist_association_name_match_retained"),
    ("table", "artist_name_checks"),
    ("index", "sqlite_autoindex_artist_name_checks_1"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(63).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0063 must retain every exact 0062 schema object without replacements"
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
        "Every additive 0063 name match object must be named exactly"
    );
    connection
}

use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &[
    "artist_name_candidates",
    "artist_review_dismissals",
    "artist_association_candidate_confirmations",
];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "artist_name_candidates"),
    ("index", "sqlite_autoindex_artist_name_candidates_1"),
    ("table", "artist_review_dismissals"),
    ("index", "sqlite_autoindex_artist_review_dismissals_1"),
    ("table", "artist_association_candidate_confirmations"),
    (
        "index",
        "sqlite_autoindex_artist_association_candidate_confirmations_1",
    ),
    (
        "trigger",
        "artist_association_candidate_confirmation_immutable",
    ),
    (
        "trigger",
        "artist_association_candidate_confirmation_retained",
    ),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(75).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0075 must not remove prior schema objects");
        assert_eq!(current, object, "0075 must only add schema objects");
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
        "Every additive 0075 object must be named exactly"
    );
    connection
}

use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["source_genres"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "source_genres"),
    ("index", "sqlite_autoindex_source_genres_1"),
    ("index", "source_genres_by_genre"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(68).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0068 must not remove prior schema objects");
        assert_eq!(current, object, "0068 must only add schema objects");
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
        "Every additive 0068 object must be named exactly"
    );
    connection
}

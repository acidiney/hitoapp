use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["listening_history", "account_history_preferences"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "listening_history"),
    ("index", "sqlite_autoindex_listening_history_1"),
    ("index", "listening_history_recent"),
    ("table", "account_history_preferences"),
    ("index", "sqlite_autoindex_account_history_preferences_1"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(85).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0085 must not remove prior schema objects");
        assert_eq!(current, object, "0085 must only add schema objects");
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
        "Every additive 0085 object must be named exactly"
    );
    connection
}

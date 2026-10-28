use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["account_pin_locks", "session_locks"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "account_pin_locks"),
    ("index", "sqlite_autoindex_account_pin_locks_1"),
    ("table", "session_locks"),
    ("index", "sqlite_autoindex_session_locks_1"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(58).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0058 must retain every exact 0057 schema object without replacements"
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
    assert_eq!(added, expected, "Every additive 0058 object must be named");
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
            "Historical accounts and sessions must not imply a PIN or a lock"
        );
    }
}

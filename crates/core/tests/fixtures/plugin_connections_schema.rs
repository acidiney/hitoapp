use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["plugin_connections"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "plugin_connections"),
    ("index", "sqlite_autoindex_plugin_connections_1"),
    ("index", "account_plugin_connections"),
    ("index", "service_plugin_connections"),
    ("index", "plugin_connection_fingerprints"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(84).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0084 must not remove prior schema objects");
        assert_eq!(current, object, "0084 must only add schema objects");
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
        "Every additive 0084 object must be named exactly"
    );
    connection
}

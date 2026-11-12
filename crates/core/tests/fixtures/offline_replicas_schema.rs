use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &[
    "erased_replica_profiles",
    "replica_activations",
    "replica_base_playlists",
    "replica_connections",
    "replica_outbox",
    "replica_received_events",
    "replica_state",
];

pub const ADDED: &[(&str, &str)] = &[
    ("index", "replica_credential_fingerprints"),
    ("index", "replica_pending_playlists"),
    ("index", "sqlite_autoindex_erased_replica_profiles_1"),
    ("index", "sqlite_autoindex_replica_activations_1"),
    ("index", "sqlite_autoindex_replica_base_playlists_1"),
    ("index", "sqlite_autoindex_replica_connections_1"),
    ("index", "sqlite_autoindex_replica_connections_2"),
    ("index", "sqlite_autoindex_replica_outbox_1"),
    ("index", "sqlite_autoindex_replica_received_events_1"),
    ("table", "erased_replica_profiles"),
    ("table", "replica_activations"),
    ("table", "replica_base_playlists"),
    ("table", "replica_connections"),
    ("table", "replica_outbox"),
    ("table", "replica_received_events"),
    ("table", "replica_state"),
    ("trigger", "replica_operation_immutable"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(92).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0092 must not remove prior schema objects");
        assert_eq!(current, object, "0092 must only add schema objects");
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
        "Every additive 0092 object must be named exactly"
    );
    let violations: Vec<String> =
        sqlx::query_scalar("SELECT \"table\" FROM pragma_foreign_key_check")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert!(
        violations.is_empty(),
        "0092 must preserve foreign key integrity"
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
            "Historical state must not create a replica in {table}"
        );
    }
}

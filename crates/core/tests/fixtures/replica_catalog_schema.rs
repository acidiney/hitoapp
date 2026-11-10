use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &[
    "replica_catalog_installs",
    "replica_catalog_facts",
    "replica_catalog_heads",
];
pub const ADDED: &[(&str, &str)] = &[
    ("table", "replica_catalog_installs"),
    ("table", "replica_catalog_facts"),
    ("table", "replica_catalog_heads"),
    ("index", "replica_catalog_account"),
    ("index", "sqlite_autoindex_replica_catalog_installs_1"),
    ("index", "sqlite_autoindex_replica_catalog_facts_1"),
    ("index", "sqlite_autoindex_replica_catalog_facts_2"),
    ("index", "sqlite_autoindex_replica_catalog_heads_1"),
    ("index", "sqlite_autoindex_replica_catalog_heads_2"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(94).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0094 must preserve prior schema objects"
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
    assert_eq!(added, expected);
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
            "historical state must not create a catalog cache in {table}"
        );
    }
}

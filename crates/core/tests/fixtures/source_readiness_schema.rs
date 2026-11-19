use sqlx::SqliteConnection;

pub const ADDED: &[(&str, &str)] = &[
    ("table", "source_readiness_jobs"),
    ("index", "sqlite_autoindex_source_readiness_jobs_1"),
    ("index", "source_readiness_jobs_due"),
    ("table", "source_readiness_checks"),
    ("index", "sqlite_autoindex_source_readiness_checks_1"),
    ("index", "source_readiness_check_recording"),
    ("table", "account_source_readiness_preferences"),
    (
        "index",
        "sqlite_autoindex_account_source_readiness_preferences_1",
    ),
];

fn added(object: &super::SchemaObject) -> bool {
    ADDED
        .iter()
        .any(|(kind, name)| object.0 == *kind && object.1 == *name)
}

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    assert!(!before.iter().any(added));
    sqlx::raw_sql(include_str!("../../migrations/0100_source_readiness.sql"))
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

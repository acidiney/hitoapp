use sqlx::SqliteConnection;

pub const ADDED: &[(&str, &str)] = &[
    ("table", "community_corrections"),
    ("index", "sqlite_autoindex_community_corrections_1"),
    ("index", "community_correction_queue"),
    ("index", "community_correction_account"),
    ("index", "community_correction_session"),
    ("trigger", "community_correction_identity"),
    ("trigger", "community_corrections_disabled"),
    ("trigger", "community_corrections_logout"),
    ("trigger", "community_corrections_session_changed"),
];

fn added(object: &super::SchemaObject) -> bool {
    ADDED
        .iter()
        .any(|(kind, name)| object.0 == *kind && object.1 == *name)
}

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    assert!(!before.iter().any(added));
    sqlx::raw_sql(include_str!(
        "../../migrations/0099_community_corrections.sql"
    ))
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

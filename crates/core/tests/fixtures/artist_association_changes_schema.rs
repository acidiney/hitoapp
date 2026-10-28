use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &[
    "recording_artist_association_changes",
    "recording_artist_association_receipts",
];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "recording_artist_association_changes"),
    (
        "index",
        "sqlite_autoindex_recording_artist_association_changes_1",
    ),
    ("index", "recording_artist_association_change_association"),
    ("trigger", "recording_artist_association_change_scope"),
    ("trigger", "recording_artist_association_change_immutable"),
    ("trigger", "recording_artist_association_change_retained"),
    ("table", "recording_artist_association_receipts"),
    (
        "index",
        "sqlite_autoindex_recording_artist_association_receipts_1",
    ),
    ("trigger", "recording_artist_association_receipt_immutable"),
    ("trigger", "recording_artist_association_receipt_retained"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(57).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0057 must retain every exact 0056 schema object without replacements"
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
        "Every additive 0057 artist association schema object must be named exactly"
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
            "Historical reviews must not imply an association change or receipt"
        );
    }
}

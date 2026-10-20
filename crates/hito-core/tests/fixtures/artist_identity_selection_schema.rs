use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &[
    "account_artist_identity_groups",
    "recording_artist_identity_choices",
    "artist_identity_choice_receipts",
];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "account_artist_identity_groups"),
    ("index", "sqlite_autoindex_account_artist_identity_groups_1"),
    ("index", "sqlite_autoindex_account_artist_identity_groups_2"),
    ("index", "sqlite_autoindex_account_artist_identity_groups_3"),
    ("table", "recording_artist_identity_choices"),
    (
        "index",
        "sqlite_autoindex_recording_artist_identity_choices_1",
    ),
    ("index", "recording_artist_identity_group"),
    ("index", "recording_artist_identity_recording"),
    ("table", "artist_identity_choice_receipts"),
    (
        "index",
        "sqlite_autoindex_artist_identity_choice_receipts_1",
    ),
    ("trigger", "artist_identity_receipt_immutable"),
    ("trigger", "artist_identity_receipt_retained"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    sqlx::migrate!("./migrations")
        .run(&mut connection)
        .await
        .unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0054 must retain every exact 0053 schema object without replacements"
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
        "Every additive 0054 identity schema object must be named exactly"
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
            "Historical credits and reviewed associations must not imply accepted identity groups, choices or receipts"
        );
    }
    let sequence: Option<i64> = sqlx::query_scalar(
        "SELECT seq FROM sqlite_sequence WHERE name='artist_identity_choice_receipts'",
    )
    .fetch_optional(&mut *connection)
    .await
    .unwrap();
    assert_eq!(
        sequence, None,
        "0054 must not invent an accepted receipt sequence"
    );
    let sql: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE name='artist_identity_choice_receipts'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert!(
        sql.contains("acceptance_order INTEGER PRIMARY KEY AUTOINCREMENT"),
        "The immutable receipt ledger must assign a durable monotonic acceptance order"
    );
}

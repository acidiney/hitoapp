use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &[
    "artist_identity_changes",
    "artist_identity_change_items",
    "artist_association_change_retirements",
    "shared_artist_corrections",
    "shared_artist_correction_subjects",
    "shared_artist_correction_states",
    "shared_artist_correction_receipts",
];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "artist_identity_changes"),
    ("index", "sqlite_autoindex_artist_identity_changes_1"),
    ("index", "sqlite_autoindex_artist_identity_changes_2"),
    ("trigger", "artist_identity_change_immutable"),
    ("trigger", "artist_identity_change_retained"),
    ("table", "artist_identity_change_items"),
    ("index", "artist_identity_change_item_recording"),
    ("trigger", "artist_identity_change_item_immutable"),
    ("trigger", "artist_identity_change_item_retained"),
    ("table", "artist_association_change_retirements"),
    ("trigger", "artist_association_change_retirement_immutable"),
    ("trigger", "artist_association_change_retirement_retained"),
    ("table", "shared_artist_corrections"),
    ("index", "sqlite_autoindex_shared_artist_corrections_1"),
    ("trigger", "shared_artist_correction_immutable"),
    ("trigger", "shared_artist_correction_retained"),
    ("trigger", "shared_artist_correction_administrator"),
    ("table", "shared_artist_correction_subjects"),
    ("index", "shared_artist_correction_subject_correction"),
    ("trigger", "shared_artist_correction_subject_immutable"),
    ("trigger", "shared_artist_correction_subject_retained"),
    ("table", "shared_artist_correction_states"),
    ("trigger", "shared_artist_correction_state_immutable"),
    ("trigger", "shared_artist_correction_state_retained"),
    ("trigger", "shared_artist_correction_state_administrator"),
    ("table", "shared_artist_correction_receipts"),
    ("trigger", "shared_artist_correction_receipt_immutable"),
    ("trigger", "shared_artist_correction_receipt_retained"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(59).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0059 must retain every exact 0058 schema object without replacements"
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
        "Every additive 0059 artist correction object must be named exactly"
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
            "Historical accounts must not imply an identity split, retirement or shared correction"
        );
    }
}

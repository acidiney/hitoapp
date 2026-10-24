use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["album_edition_choices", "album_edition_choice_receipts"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "album_edition_choices"),
    ("index", "sqlite_autoindex_album_edition_choices_1"),
    ("trigger", "album_edition_choice_scope_insert"),
    ("trigger", "album_edition_choice_scope_update"),
    ("table", "album_edition_choice_receipts"),
    ("index", "sqlite_autoindex_album_edition_choice_receipts_1"),
    ("trigger", "album_edition_receipt_immutable"),
    ("trigger", "album_edition_receipt_retained"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(56).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0056 must retain every exact 0055 schema object without replacements"
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
        "Every additive 0056 album edition schema object must be named exactly"
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
            "Historical reviews and releases must not imply a remembered album edition or receipt"
        );
    }
}

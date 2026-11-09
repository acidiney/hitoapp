use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["account_recording_credit_members"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "account_recording_credit_members"),
    (
        "index",
        "sqlite_autoindex_account_recording_credit_members_1",
    ),
    ("index", "account_recording_credit_member_artist"),
    ("index", "account_recording_credit_member_recording"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(91).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0091 must not remove prior schema objects");
        assert_eq!(current, object, "0091 must only add schema objects");
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
        "Every additive 0091 object must be named exactly"
    );
    connection
}

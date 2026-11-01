use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["recording_group_members", "recording_group_splits"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "recording_group_members"),
    ("index", "sqlite_autoindex_recording_group_members_1"),
    ("index", "recording_group_members_canonical"),
    ("index", "recording_group_members_canonical_only"),
    ("trigger", "recording_group_members_flat"),
    ("trigger", "recording_group_members_immutable"),
    ("table", "recording_group_splits"),
    ("index", "sqlite_autoindex_recording_group_splits_1"),
    ("index", "recording_group_splits_other"),
    ("index", "recording_musicbrainz_observation_mbid"),
    ("index", "recording_isrc_observation_isrc"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(77).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0077 must not remove prior schema objects");
        assert_eq!(current, object, "0077 must only add schema objects");
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
        "Every additive 0077 object must be named exactly"
    );
    connection
}

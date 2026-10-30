use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &[
    "recording_isrc_observations",
    "artist_association_provider_matches",
    "artist_provider_checks",
];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "recording_isrc_observations"),
    ("index", "sqlite_autoindex_recording_isrc_observations_1"),
    ("index", "recording_isrc_observation_recording"),
    ("trigger", "recording_isrc_observation_immutable"),
    ("trigger", "recording_isrc_observation_retained"),
    ("table", "artist_association_provider_matches"),
    (
        "index",
        "sqlite_autoindex_artist_association_provider_matches_1",
    ),
    ("trigger", "artist_association_provider_match_immutable"),
    ("trigger", "artist_association_provider_match_retained"),
    ("table", "artist_provider_checks"),
    ("index", "sqlite_autoindex_artist_provider_checks_1"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(61).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0061 must retain every exact 0060 schema object without replacements"
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
        "Every additive 0061 provider evidence object must be named exactly"
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
            "Historical accounts must not imply an ISRC observation, provider match or check"
        );
    }
}

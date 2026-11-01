use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &[
    "recording_credit_observations",
    "recording_credited_artist_observations",
];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "recording_credit_observations"),
    ("index", "sqlite_autoindex_recording_credit_observations_1"),
    ("index", "recording_credit_observation_recording"),
    ("trigger", "recording_credit_observation_immutable"),
    ("table", "recording_credited_artist_observations"),
    (
        "index",
        "sqlite_autoindex_recording_credited_artist_observations_1",
    ),
    ("index", "recording_credited_artist_observation_recording"),
    ("trigger", "recording_credited_artist_observation_immutable"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(78).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0078 must not remove prior schema objects");
        assert_eq!(current, object, "0078 must only add schema objects");
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
        "Every additive 0078 object must be named exactly"
    );
    connection
}

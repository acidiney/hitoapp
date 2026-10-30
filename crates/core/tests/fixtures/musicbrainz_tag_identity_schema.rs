use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["recording_musicbrainz_observations"];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "recording_musicbrainz_observations"),
    (
        "index",
        "sqlite_autoindex_recording_musicbrainz_observations_1",
    ),
    ("index", "recording_musicbrainz_observation_recording"),
    ("trigger", "recording_musicbrainz_observation_immutable"),
    ("trigger", "recording_musicbrainz_observation_retained"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(64).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    for object in &before {
        assert!(
            after.contains(object),
            "0064 must retain every exact 0063 schema object without replacements"
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
        "Every additive 0064 MusicBrainz tag identity object must be named exactly"
    );
    connection
}

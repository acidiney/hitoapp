use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &[
    "correction_erasure_scopes",
    "shared_artist_corrections",
    "shared_artist_correction_states",
];

pub const NEW_TABLES: &[&str] = &["correction_erasure_scopes"];

pub const REPLACED: &[(&str, &str)] = &[
    ("table", "shared_artist_corrections"),
    ("table", "shared_artist_correction_states"),
    ("trigger", "shared_artist_correction_immutable"),
    ("trigger", "shared_artist_correction_state_immutable"),
    ("trigger", "shared_artist_correction_subject_retained"),
    ("trigger", "shared_artist_correction_receipt_retained"),
    ("trigger", "recording_artist_association_retained"),
    ("trigger", "recording_artist_association_change_retained"),
    ("trigger", "recording_artist_association_receipt_retained"),
    (
        "trigger",
        "recording_artist_association_change_order_retained",
    ),
    ("trigger", "artist_association_change_retirement_retained"),
    ("trigger", "artist_identity_change_retained"),
    ("trigger", "artist_identity_change_item_retained"),
    ("trigger", "artist_identity_receipt_retained"),
    ("trigger", "album_edition_receipt_retained"),
    ("trigger", "artist_association_match_retained"),
    ("trigger", "artist_association_provider_match_retained"),
    ("trigger", "artist_association_name_match_retained"),
    ("trigger", "recording_isrc_observation_retained"),
    ("trigger", "recording_artist_url_observation_retained"),
    ("trigger", "recording_musicbrainz_observation_retained"),
];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "correction_erasure_scopes"),
    ("index", "sqlite_autoindex_correction_erasure_scopes_1"),
    ("trigger", "shared_artist_correction_source"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(65).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    let mut replaced = Vec::new();
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0065 must not remove prior schema objects");
        if current != object {
            replaced.push((object.0.as_str(), object.1.as_str()));
        }
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
    let (mut expected_replaced, mut expected_added) = (REPLACED.to_vec(), ADDED.to_vec());
    replaced.sort_unstable();
    added.sort_unstable();
    expected_replaced.sort_unstable();
    expected_added.sort_unstable();
    assert_eq!(
        replaced, expected_replaced,
        "only the named 0065 correction tables and triggers may be replaced"
    );
    assert_eq!(
        added, expected_added,
        "Every additive 0065 erasure object must be named exactly"
    );
    connection
}

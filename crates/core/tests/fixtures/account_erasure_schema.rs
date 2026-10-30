use sqlx::SqliteConnection;

pub const TABLES: &[&str] = &["erased_download_files"];

pub const NEW_TABLES: &[&str] = TABLES;

pub const REPLACED: &[(&str, &str)] = &[
    ("trigger", "download_receipt_retained"),
    ("trigger", "download_retention_retained"),
    ("trigger", "download_playlist_receipt_retained"),
    ("trigger", "download_terminal_failure_retained"),
    ("trigger", "provider_item_retained"),
    ("trigger", "source_resolution_receipt_retained"),
    ("trigger", "automatic_source_decision_retained"),
    ("trigger", "source_choice_receipt_retained"),
    ("trigger", "source_choice_reset_receipt_retained"),
    ("trigger", "podcast_episode_retained"),
    ("trigger", "podcast_request_account_count_retained"),
    ("trigger", "podcast_subscription_receipt_retained"),
    ("trigger", "podcast_refresh_receipt_retained"),
    ("trigger", "podcast_unsubscribe_receipt_retained"),
    ("trigger", "podcast_source_mapping_retained"),
    ("trigger", "podcast_source_retained"),
    ("trigger", "playlist_metadata_receipt_retained"),
];

pub const ADDED: &[(&str, &str)] = &[
    ("table", "erased_download_files"),
    ("index", "sqlite_autoindex_erased_download_files_1"),
    ("index", "erased_download_files_host"),
];

pub async fn reference(mut connection: SqliteConnection) -> SqliteConnection {
    let before = super::schema(&mut connection).await;
    super::through(67).run(&mut connection).await.unwrap();
    let after = super::schema(&mut connection).await;
    let mut replaced = Vec::new();
    for object in &before {
        let current = after
            .iter()
            .find(|current| current.0 == object.0 && current.1 == object.1)
            .expect("0067 must not remove prior schema objects");
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
        "only the named 0067 retention triggers may be replaced"
    );
    assert_eq!(
        added, expected_added,
        "Every additive 0067 account erasure object must be named exactly"
    );
    connection
}

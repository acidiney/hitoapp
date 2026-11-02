#![cfg(unix)]

use hito_core::test_support::{media_tool, support};

#[path = "../fixtures/id3_tags.rs"]
mod id3;

use hito_core::{CoreConfig, HostDeviceId, MetadataHistoryQuery, Search};
use id3::{Frame, Layout, Version, tag_with, write_mp3_around};
use sqlx::sqlite::SqliteConnectOptions;
use std::path::Path;

const PRIOR_VERSION: i64 = 7;
const CURRENT_VERSION: i64 = 8;

async fn represent_prior_version(database: &Path) {
    let pool = sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(database)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    for table in ["metadata_observations", "source_metadata"] {
        let updated = sqlx::query(&format!(
            "UPDATE {table} SET inspection_version = ? WHERE inspection_version = ?"
        ))
        .bind(PRIOR_VERSION)
        .bind(CURRENT_VERSION)
        .execute(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(updated.rows_affected(), 1, "{table}");
    }
    transaction.commit().await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn a_new_inspection_version_reinspects_unchanged_files_once_without_decoding_audio() {
    let (dir, initial, actor, library, music) = support::library("rescan-host").await;
    let footer = Layout {
        footer: true,
        ..Layout::default()
    };
    let tag = tag_with(
        Version::V24,
        footer,
        &[Frame::text("TIT2", "Appended title")],
    );
    let path = music.join("private-file-name.mp3");
    write_mp3_around(dir.path(), &path, &[], &tag);
    support::scan(&initial, &actor, &library).await;
    let recording = initial
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    assert_eq!(recording.title, "Appended title");
    let original = std::fs::read(&path).unwrap();
    drop(initial);
    let database = dir.path().join("hito.db");
    represent_prior_version(&database).await;

    let probes = dir.path().join("metadata-probes");
    let decodes = dir.path().join("audio-decodes");
    let mut config = CoreConfig::new(database, music, HostDeviceId::new("rescan-host").unwrap());
    config.media_tools.ffprobe = media_tool::script(
        dir.path(),
        "ffprobe",
        &format!(
            "printf 'probe\\n' >> {}\nexec /usr/bin/ffprobe \"$@\"",
            media_tool::quote(&probes),
        ),
    );
    config.media_tools.ffmpeg = media_tool::script(
        dir.path(),
        "ffmpeg",
        &format!(
            "printf 'decode\\n' >> {}\nexit 91",
            media_tool::quote(&decodes),
        ),
    );
    let hito = hito_core::open(config).await.unwrap();
    let versions = |observations: &hito_core::RecordingMetadata| {
        let mut versions: Vec<_> = observations
            .observations
            .items
            .iter()
            .map(|observation| observation.inspection_version)
            .collect();
        versions.sort();
        versions
    };
    let before = hito
        .catalog
        .recording_metadata(&actor, &recording.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(versions(&before), vec![Some(PRIOR_VERSION)]);

    support::scan(&hito, &actor, &library).await;
    assert_eq!(std::fs::read_to_string(&probes).unwrap().lines().count(), 1);
    assert!(!decodes.exists(), "The audio proof must be reused");
    let after = hito
        .catalog
        .recording_metadata(&actor, &recording.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(
        versions(&after),
        vec![Some(PRIOR_VERSION), Some(CURRENT_VERSION)]
    );
    assert_eq!(
        after
            .selected_observation
            .as_ref()
            .unwrap()
            .inspection_version,
        Some(CURRENT_VERSION)
    );

    support::scan(&hito, &actor, &library).await;
    support::scan(&hito, &actor, &library).await;
    assert_eq!(std::fs::read_to_string(&probes).unwrap().lines().count(), 1);
    assert!(!decodes.exists());
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

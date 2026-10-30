#[path = "../fixtures/legacy_library.rs"]
mod legacy;
use hito_core::test_support::support;

use hito_core::{MetadataHistoryQuery, Search};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, write_wav};

#[tokio::test]
async fn source_observations_keep_revisions_and_distinguish_tags_from_fallbacks() {
    let (_dir, hito, actor, library, music) = library("observation-host").await;
    let path = music.join("Fallback.wav");
    write_wav(&path, 0);
    scan(&hito, &actor, &library).await;
    let recording = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let first = hito
        .catalog
        .recording_metadata(&actor, &recording, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(first.title.inherited, "Fallback");
    let first_observation = first.selected_observation.unwrap();
    assert!(first_observation.tags.as_ref().unwrap().title.is_none());
    assert_eq!(first.observations.items.len(), 1);
    scan(&hito, &actor, &library).await;
    assert_eq!(
        hito.catalog
            .recording_metadata(&actor, &recording, MetadataHistoryQuery::default())
            .await
            .unwrap()
            .observations
            .items
            .len(),
        1
    );
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Tagged title".into());
    tag.set_artist("Tagged performer".into());
    tag.set_track(3);
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    scan(&hito, &actor, &library).await;
    let latest = hito
        .catalog
        .recording_metadata(
            &actor,
            &recording,
            MetadataHistoryQuery {
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(latest.title.inherited, "Tagged title");
    let selected = latest.selected_observation.unwrap();
    assert_ne!(selected.id, first_observation.id);
    assert_eq!(selected.tags.unwrap().title.unwrap().value, "Tagged title");
    assert_eq!(selected.track_number, Some(3));
    assert_eq!(latest.observations.items.len(), 1);
    let older = hito
        .catalog
        .recording_metadata(
            &actor,
            &recording,
            MetadataHistoryQuery {
                before: latest.observations.next_before,
                limit: Some(1),
            },
        )
        .await
        .unwrap();
    assert_eq!(older.observations.items[0].id, first_observation.id);
    assert_eq!(older.observations.items[0].title, "Fallback");
    assert!(older.observations.next_before.is_none());
    std::fs::remove_file(&path).unwrap();
    scan(&hito, &actor, &library).await;
    let missing = hito
        .catalog
        .recording_metadata(&actor, &recording, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(missing.selected_observation.unwrap().id, selected.id);
    assert_eq!(missing.title.inherited, "Tagged title");
    let json = serde_json::to_string(&missing.observations).unwrap();
    assert!(!json.contains(music.to_str().unwrap()));
    assert!(!json.contains("file_checksum"));
}

#[tokio::test]
async fn upgraded_metadata_reports_its_unknown_tag_detail_without_inventing_history() {
    let fixture = legacy::Legacy::seed().await;
    let original = std::fs::read(&fixture.original).unwrap();
    let (hito, actor) = fixture.open().await;
    let detail = hito
        .catalog
        .recording_metadata(&actor, legacy::RECORDING, MetadataHistoryQuery::default())
        .await
        .unwrap();
    let observation = detail.selected_observation.unwrap();
    assert!(matches!(
        observation.origin,
        hito_core::ObservationOrigin::LegacyNormalized
    ));
    assert_eq!(observation.title, "Legacy track");
    assert!(observation.tags.is_none() && observation.observed_at.is_none());
    scan(&hito, &actor, legacy::LIBRARY).await;
    let detail = hito
        .catalog
        .recording_metadata(&actor, legacy::RECORDING, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert!(matches!(
        detail.selected_observation.unwrap().origin,
        hito_core::ObservationOrigin::FileInspection
    ));
    assert_eq!(detail.observations.items.len(), 2);
    assert_eq!(std::fs::read(&fixture.original).unwrap(), original);
}

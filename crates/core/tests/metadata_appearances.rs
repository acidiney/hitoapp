use hito_core::test_support::support;

use hito_core::{Action, Browse, MetadataHistoryQuery, Search};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, synchronize, write_wav};
use uuid::Uuid;

#[tokio::test]
async fn personal_title_covers_release_appearances_and_reset_restores_each_observation() {
    let (_dir, hito, actor, library, music) = library("appearance-host").await;
    for title in ["Opening", "Encore"] {
        let folder = music.join(title);
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("same.wav");
        write_wav(&path, 0);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(title.into());
        tag.set_album(title.into());
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }
    scan(&hito, &actor, &library).await;
    let track = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording = Uuid::parse_str(&track.id).unwrap();
    synchronize(
        &hito,
        &actor,
        vec![Action::RecordingTitleSet {
            recording_id: recording,
            title: "One performance".into(),
        }],
    )
    .await;
    let releases = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(releases.total, 2);
    for release in &releases.items {
        let detail = hito
            .catalog
            .release(&actor, &release.id, Browse::default())
            .await
            .unwrap();
        assert_eq!(detail.tracks.items[0].recording.title, "One performance");
        assert_eq!(
            detail.tracks.items[0].recording.inherited_title,
            release.title
        );
    }
    synchronize(
        &hito,
        &actor,
        vec![Action::RecordingTitleInherit {
            recording_id: recording,
        }],
    )
    .await;
    for release in &releases.items {
        let detail = hito
            .catalog
            .release(&actor, &release.id, Browse::default())
            .await
            .unwrap();
        assert_eq!(detail.tracks.items[0].recording.title, release.title);
    }
    let selected = hito
        .catalog
        .recording_metadata(&actor, &track.id, MetadataHistoryQuery::default())
        .await
        .unwrap()
        .selected_observation
        .unwrap();
    std::fs::remove_file(music.join(&selected.title).join("same.wav")).unwrap();
    scan(&hito, &actor, &library).await;
    let detail = hito
        .catalog
        .recording_metadata(&actor, &track.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_ne!(detail.selected_observation.unwrap().id, selected.id);
    assert_ne!(detail.title.inherited, selected.title);
    assert_eq!(detail.observations.items.len(), 2);
}

use hito_core::test_support::support;

use hito_core::{Browse, Error, MetadataHistoryQuery};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, write_wav};

#[tokio::test]
async fn album_history_is_paged_within_its_authorized_release() {
    let (_dir, hito, actor, library, music) = library("release-history-host").await;
    let mut tag = Tag::new(TagType::Id3v2);
    for (index, album) in ["First", "Other"].into_iter().enumerate() {
        let folder = music.join(album);
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("track.wav");
        write_wav(&path, index as i16);
        tag.set_album(album.into());
        tag.set_title("Original track".into());
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }
    scan(&hito, &actor, &library).await;
    let album = hito
        .catalog
        .releases(
            &actor,
            Browse {
                q: Some("First".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .items
        .remove(0);
    tag.set_album("First".into());
    tag.set_title("Retagged track".into());
    tag.save_to_path(music.join("First/track.wav"), WriteOptions::default())
        .unwrap();
    scan(&hito, &actor, &library).await;
    let latest = hito
        .catalog
        .release_metadata(
            &actor,
            &album.id,
            MetadataHistoryQuery {
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(latest.observations.items.len(), 1);
    assert_eq!(latest.observations.items[0].title, "Retagged track");
    let older = hito
        .catalog
        .release_metadata(
            &actor,
            &album.id,
            MetadataHistoryQuery {
                before: latest.observations.next_before,
                limit: Some(1),
            },
        )
        .await
        .unwrap();
    assert_eq!(older.observations.items[0].title, "Original track");
    assert_eq!(
        older.observations.items[0].release_id.as_deref(),
        Some(album.id.as_str())
    );
    assert!(older.observations.next_before.is_none());
    assert!(older.title.personal.is_none() && older.artist.personal.is_none());
    hito.identity
        .set_library_access(&actor, &library, &actor.user().id, false)
        .await
        .unwrap();
    assert!(matches!(
        hito.catalog
            .release_metadata(&actor, &album.id, MetadataHistoryQuery::default())
            .await,
        Err(Error::NotFound)
    ));
}

mod support;

use hito_core::{Action, Browse, MetadataHistoryQuery};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, synchronize, write_wav};
use uuid::Uuid;

#[tokio::test]
async fn equal_personal_album_names_stay_distinct_and_each_field_resets_independently() {
    let (_dir, hito, actor, library, music) = library("release-reset-host").await;
    for name in ["First", "Second"] {
        let folder = music.join(name);
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("same.wav");
        write_wav(&path, 0);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Track".into());
        tag.set_artist("Performer".into());
        tag.set_album(name.into());
        tag.insert_text(ItemKey::AlbumArtist, format!("Album credit {name}"));
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }
    scan(&hito, &actor, &library).await;
    let albums = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap()
        .items;
    let first = Uuid::parse_str(&albums[0].id).unwrap();
    for release in &albums {
        let id = Uuid::parse_str(&release.id).unwrap();
        synchronize(
            &hito,
            &actor,
            vec![
                Action::ReleaseTitleSet {
                    release_id: id,
                    title: "Same display name".into(),
                },
                Action::ReleaseArtistSet {
                    release_id: id,
                    artist: "Same private album credit".into(),
                },
            ],
        )
        .await;
    }
    let renamed = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(renamed.total, 2);
    assert_ne!(renamed.items[0].id, renamed.items[1].id);
    assert_eq!(renamed.items[0].artist_id, renamed.items[1].artist_id);
    assert_eq!(
        hito.catalog
            .artist(
                &actor,
                renamed.items[0].artist_id.as_deref().unwrap(),
                Browse::default()
            )
            .await
            .unwrap()
            .artist
            .release_count,
        2
    );
    scan(&hito, &actor, &library).await;
    assert!(
        hito.catalog
            .releases(&actor, Browse::default())
            .await
            .unwrap()
            .items
            .iter()
            .all(|album| album.title == "Same display name")
    );
    synchronize(
        &hito,
        &actor,
        vec![Action::ReleaseTitleInherit { release_id: first }],
    )
    .await;
    let detail = hito
        .catalog
        .release_metadata(&actor, &albums[0].id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(detail.title.value, "First");
    assert!(detail.title.personal.is_none());
    assert_eq!(
        detail.artist.personal.as_deref(),
        Some("Same private album credit")
    );
    synchronize(
        &hito,
        &actor,
        vec![Action::ReleaseArtistInherit { release_id: first }],
    )
    .await;
    let detail = hito
        .catalog
        .release(&actor, &albums[0].id, Browse::default())
        .await
        .unwrap();
    assert_eq!(detail.release.title, "First");
    assert_eq!(detail.release.artist.as_deref(), Some("Album credit First"));
    assert_eq!(
        hito.catalog
            .artist(
                &actor,
                renamed.items[0].artist_id.as_deref().unwrap(),
                Browse::default()
            )
            .await
            .unwrap()
            .artist
            .release_count,
        1
    );
    let second = hito
        .catalog
        .release_metadata(&actor, &albums[1].id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(second.title.personal.as_deref(), Some("Same display name"));
    assert_eq!(
        second.artist.personal.as_deref(),
        Some("Same private album credit")
    );
}

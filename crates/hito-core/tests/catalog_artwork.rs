mod support;

use hito_core::Browse;
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, write_wav};

#[tokio::test]
async fn album_artwork_uses_its_own_release_when_audio_identity_is_shared() {
    let (_dir, hito, actor, library, music) = library("cover-host").await;
    let covers: [(&str, &[u8]); 2] = [
        ("Album A", include_bytes!("fixtures/cover-a.png")),
        ("Album B", include_bytes!("fixtures/cover-b.png")),
    ];
    for (title, cover) in covers {
        let directory = music.join(title);
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("same.wav");
        write_wav(&file, 0);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_album(title.into());
        tag.save_to_path(file, WriteOptions::default()).unwrap();
        std::fs::write(directory.join("cover.png"), cover).unwrap();
    }
    scan(&hito, &actor, &library).await;
    let albums = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(
        albums.items[0].artwork_recording_id,
        albums.items[1].artwork_recording_id
    );
    let first = albums
        .items
        .iter()
        .find(|album| album.title == "Album A")
        .unwrap()
        .id
        .clone();
    for album in albums.items {
        let actual = hito
            .catalog
            .release_artwork(&actor, &album.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            actual.bytes,
            covers
                .iter()
                .find(|(title, _)| *title == album.title)
                .unwrap()
                .1
        );
    }
    std::fs::remove_file(music.join("Album A/same.wav")).unwrap();
    scan(&hito, &actor, &library).await;
    assert!(
        hito.catalog
            .release_artwork(&actor, &first)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        hito.catalog
            .release(&actor, &first, Browse::default())
            .await
            .unwrap()
            .tracks
            .items[0]
            .recording
            .available
    );
}

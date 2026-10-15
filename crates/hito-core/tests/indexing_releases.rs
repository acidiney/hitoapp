mod support;

use hito_core::Search;
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::path::Path;
use support::{library, scan, write_wav};

fn tag_wav(path: &Path, title: &str, artist: &str, album: &str, album_artist: &str) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(artist.into());
    tag.set_album(album.into());
    tag.insert_text(ItemKey::AlbumArtist, album_artist.into());
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

#[tokio::test]
async fn tagged_albums_in_one_folder_stay_distinct_and_retags_resolve_the_release() {
    let (_dir, hito, actor, library, music) = library("server").await;
    for (offset, (name, title, artist, album, album_artist)) in [
        ("a.wav", "First", "Singer one", "Album one", "Band one"),
        ("b.wav", "Second", "Singer two", "Album two", "Band one"),
        ("c.wav", "Third", "Singer three", "Album one", "Band two"),
        ("d.wav", "Fourth", "Singer four", "Album one", "Band one"),
    ]
    .into_iter()
    .enumerate()
    {
        let path = music.join(name);
        write_wav(&path, offset as i16);
        tag_wav(&path, title, artist, album, album_artist);
    }
    scan(&hito, &actor, &library).await;

    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 4);
    let first = recordings
        .items
        .iter()
        .find(|r| r.title == "First")
        .unwrap();
    let second = recordings
        .items
        .iter()
        .find(|r| r.title == "Second")
        .unwrap();
    let third = recordings
        .items
        .iter()
        .find(|r| r.title == "Third")
        .unwrap();
    let fourth = recordings
        .items
        .iter()
        .find(|r| r.title == "Fourth")
        .unwrap();
    assert_eq!(first.album.as_deref(), Some("Album one"));
    assert_eq!(second.album.as_deref(), Some("Album two"));
    assert_eq!(third.album.as_deref(), Some("Album one"));
    assert_eq!(first.artist, "Singer one");
    assert_eq!(fourth.artist, "Singer four");
    assert!(first.release_id.is_some());
    assert_ne!(first.release_id, second.release_id);
    assert_ne!(first.release_id, third.release_id);
    assert_eq!(first.release_id, fourth.release_id);
    let original_source = hito
        .catalog
        .sources(&actor, &first.id)
        .await
        .unwrap()
        .remove(0)
        .id;
    let original_bytes = std::fs::read(music.join("a.wav")).unwrap();

    tag_wav(
        &music.join("a.wav"),
        "First",
        "Singer one",
        "Album two",
        "Band one",
    );
    assert_ne!(std::fs::read(music.join("a.wav")).unwrap(), original_bytes);
    scan(&hito, &actor, &library).await;

    let rescanned = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    let retagged = rescanned
        .items
        .iter()
        .find(|r| r.title == "First" && r.available)
        .unwrap();
    assert_eq!(retagged.album.as_deref(), Some("Album two"));
    assert_eq!(retagged.id, first.id);
    assert_eq!(retagged.release_id, second.release_id);
    let source = hito
        .catalog
        .sources(&actor, &retagged.id)
        .await
        .unwrap()
        .remove(0);
    assert!(source.ready);
    assert_eq!(source.id, original_source);
    assert_eq!(
        support::audio_bytes(hito.catalog.audio_file(&actor, &source.id).await.unwrap()),
        std::fs::read(music.join("a.wav")).unwrap()
    );
    for original in [second, third, fourth] {
        let current = rescanned
            .items
            .iter()
            .find(|r| r.id == original.id)
            .unwrap();
        assert!(current.available);
        assert_eq!(current.album, original.album);
        assert_eq!(current.release_id, original.release_id);
    }
}

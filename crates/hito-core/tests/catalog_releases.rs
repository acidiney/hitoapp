mod support;

use hito_core::Browse;
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, write_wav};

#[tokio::test]
async fn album_pages_preserve_each_release_appearance_of_the_same_recording() {
    let (_dir, hito, actor, library, music) = library("album-host").await;
    for (folder, album) in [("original", "Original album"), ("collection", "Collection")] {
        let directory = music.join(folder);
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("same.wav");
        write_wav(&file, 0);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("One performance".into());
        tag.set_artist("Performer".into());
        tag.set_album(album.into());
        tag.insert_text(ItemKey::AlbumArtist, "Album credit".into());
        tag.save_to_path(file, WriteOptions::default()).unwrap();
    }
    scan(&hito, &actor, &library).await;

    let albums = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(albums.total, 2);
    let mut recordings = Vec::new();
    for album in albums.items {
        assert_eq!(album.track_count, 1);
        assert_eq!(album.artist, "Album credit");
        let detail = hito
            .catalog
            .release(&actor, &album.id, Browse::default())
            .await
            .unwrap();
        assert_eq!(detail.release.id, album.id);
        assert_eq!(detail.tracks.total, 1);
        assert_eq!(
            detail.tracks.items[0].recording.album.as_deref(),
            Some(album.title.as_str())
        );
        assert!(detail.tracks.items[0].recording.available);
        recordings.push(detail.tracks.items[0].recording.id.clone());
    }
    assert_eq!(recordings[0], recordings[1]);
}

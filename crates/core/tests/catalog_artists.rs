use hito_core::test_support::support;

use hito_core::Browse;
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use support::{library, scan, write_wav};

#[tokio::test]
async fn artist_pages_keep_exact_credits_and_album_appearances_without_guessing_people() {
    let (_dir, hito, actor, library, music) = library("artist-host").await;
    for (offset, credit) in ["Echo", "echo", "Echo feat. Guest"].into_iter().enumerate() {
        let file = music.join(format!("{offset}.wav"));
        write_wav(&file, offset as i16);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(format!("Track {offset}"));
        tag.set_artist(credit.into());
        tag.set_album("Compilation".into());
        tag.insert_text(ItemKey::AlbumArtist, "Various Artists".into());
        tag.save_to_path(file, WriteOptions::default()).unwrap();
    }
    scan(&hito, &actor, &library).await;
    let artists = hito
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 4);
    let mut ids = Vec::new();
    for artist in artists.items {
        let page = hito
            .catalog
            .artist(&actor, &artist.id, Browse::default())
            .await
            .unwrap();
        assert_eq!(page.artist.name, artist.name);
        assert_eq!(page.releases.total, 1);
        assert_eq!(page.releases.items[0].title, "Compilation");
        let tracks = hito
            .catalog
            .recordings(
                &actor,
                hito_core::Search {
                    artist: Some(artist.id.clone()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            tracks.total,
            if artist.name == "Various Artists" {
                0
            } else {
                1
            }
        );
        ids.push(artist.id);
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 4);
}

#[tokio::test]
async fn artist_and_album_queries_treat_search_as_literal_private_input() {
    let (_dir, hito, actor, library, music) = library("artist-host").await;
    write_wav(&music.join("track.wav"), 0);
    scan(&hito, &actor, &library).await;
    for q in ["Unknown", "x') OR 1=1 --"] {
        let artists = hito
            .catalog
            .artists(
                &actor,
                Browse {
                    q: Some(q.into()),
                    ..Browse::default()
                },
            )
            .await
            .unwrap();
        let releases = hito
            .catalog
            .releases(
                &actor,
                Browse {
                    q: Some(q.into()),
                    ..Browse::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(artists.total, if q == "Unknown" { 1 } else { 0 });
        assert_eq!(releases.total, artists.total);
    }
}

#[tokio::test]
async fn search_folds_unicode_without_changing_exact_artist_identity() {
    let (_dir, hito, actor, library, music) = library("artist-host").await;
    let file = music.join("track.wav");
    write_wav(&file, 0);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_artist("Björk".into());
    tag.set_album("CÉU".into());
    tag.save_to_path(file, WriteOptions::default()).unwrap();
    scan(&hito, &actor, &library).await;
    let artists = hito
        .catalog
        .artists(
            &actor,
            Browse {
                q: Some("BJÖRK".into()),
                ..Browse::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(artists.total, 1);
    assert_eq!(artists.items[0].name, "Björk");
    assert_eq!(
        hito.catalog
            .releases(
                &actor,
                Browse {
                    q: Some("céu".into()),
                    ..Browse::default()
                }
            )
            .await
            .unwrap()
            .total,
        1
    );
}

#[tokio::test]
async fn retagging_updates_artist_appearances_without_replacing_the_recording() {
    let (_dir, hito, actor, library, music) = library("artist-host").await;
    let file = music.join("track.wav");
    write_wav(&file, 0);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("One recording".into());
    tag.set_artist("Original credit".into());
    tag.set_album("Album".into());
    tag.save_to_path(&file, WriteOptions::default()).unwrap();
    scan(&hito, &actor, &library).await;
    let before = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording = hito
        .catalog
        .release(&actor, &before.id, Browse::default())
        .await
        .unwrap()
        .tracks
        .items
        .remove(0)
        .recording
        .id;
    tag.set_artist("Corrected credit".into());
    tag.save_to_path(&file, WriteOptions::default()).unwrap();
    scan(&hito, &actor, &library).await;
    let artists = hito
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 1);
    assert_eq!(artists.items[0].name, "Corrected credit");
    let artist = hito
        .catalog
        .artist(&actor, &artists.items[0].id, Browse::default())
        .await
        .unwrap();
    let current = hito
        .catalog
        .release(&actor, &artist.releases.items[0].id, Browse::default())
        .await
        .unwrap();
    assert_eq!(current.tracks.items[0].recording.id, recording);
}

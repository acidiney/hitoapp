mod support;

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

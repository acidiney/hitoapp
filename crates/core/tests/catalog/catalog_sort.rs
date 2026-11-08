use hito_core::test_support::support;

use hito_core::{Actor, Browse, CatalogSort, Error, Search, Services};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::path::Path;
use support::{library, scan, write_wav};

struct Song {
    title: &'static str,
    artist: &'static str,
    album: &'static str,
    date: Option<&'static str>,
    audio: i16,
}

fn write_song(music: &Path, song: &Song) {
    let file = music.join(format!("{}.wav", song.title));
    write_wav(&file, song.audio);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(song.title.into());
    tag.set_artist(song.artist.into());
    tag.set_album(song.album.into());
    tag.insert_text(ItemKey::AlbumArtist, song.artist.into());
    if let Some(date) = song.date {
        tag.insert_text(ItemKey::RecordingDate, date.into());
    }
    tag.save_to_path(file, WriteOptions::default()).unwrap();
}

const OLD: Song = Song {
    title: "Bravo",
    artist: "Zed",
    album: "Old album",
    date: Some("1999-05-04"),
    audio: 1,
};
const NEW: Song = Song {
    title: "alpha",
    artist: "Yan",
    album: "New album",
    date: Some("2021"),
    audio: 2,
};
const UNDATED: Song = Song {
    title: "Charlie",
    artist: "Xi",
    album: "Undated album",
    date: None,
    audio: 3,
};

async fn added_in_two_scans() -> (tempfile::TempDir, Services, Actor) {
    let (dir, hito, actor, library, music) = library("sort-host").await;
    write_song(&music, &OLD);
    write_song(&music, &NEW);
    scan(&hito, &actor, &library).await;
    write_song(&music, &UNDATED);
    scan(&hito, &actor, &library).await;
    (dir, hito, actor)
}

async fn track_titles(
    hito: &Services,
    actor: &Actor,
    sort: CatalogSort,
    limit: i64,
) -> Vec<String> {
    let mut titles = Vec::new();
    let mut offset = 0;
    loop {
        let page = hito
            .catalog
            .recordings(
                actor,
                Search {
                    sort: Some(sort),
                    offset: Some(offset),
                    limit: Some(limit),
                    ..Search::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.total, 3);
        if page.items.is_empty() {
            return titles;
        }
        offset += page.items.len() as i64;
        titles.extend(page.items.into_iter().map(|item| item.title));
    }
}

async fn album_titles(
    hito: &Services,
    actor: &Actor,
    sort: CatalogSort,
    limit: i64,
) -> Vec<String> {
    let mut titles = Vec::new();
    let mut offset = 0;
    loop {
        let page = hito
            .catalog
            .releases(
                actor,
                Browse {
                    sort: Some(sort),
                    offset: Some(offset),
                    limit: Some(limit),
                    ..Browse::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.total, 3);
        if page.items.is_empty() {
            return titles;
        }
        offset += page.items.len() as i64;
        titles.extend(page.items.into_iter().map(|item| item.title));
    }
}

async fn artist_names(
    hito: &Services,
    actor: &Actor,
    sort: CatalogSort,
    limit: i64,
) -> Vec<String> {
    let mut names = Vec::new();
    let mut offset = 0;
    loop {
        let page = hito
            .catalog
            .artists(
                actor,
                Browse {
                    sort: Some(sort),
                    offset: Some(offset),
                    limit: Some(limit),
                    ..Browse::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.total, 3);
        if page.items.is_empty() {
            return names;
        }
        offset += page.items.len() as i64;
        names.extend(page.items.into_iter().map(|item| item.name));
    }
}

#[tokio::test]
async fn tracks_sort_by_recently_added_title_or_artist_and_page_without_gaps() {
    let (_dir, hito, actor) = added_in_two_scans().await;
    for limit in [1, 2, 200] {
        let recent = track_titles(&hito, &actor, CatalogSort::Recent, limit).await;
        assert_eq!(recent[0], "Charlie");
        let mut earlier = recent[1..].to_vec();
        earlier.sort();
        assert_eq!(earlier, ["Bravo", "alpha"]);
        assert_eq!(
            track_titles(&hito, &actor, CatalogSort::Title, limit).await,
            ["alpha", "Bravo", "Charlie"]
        );
        assert_eq!(
            track_titles(&hito, &actor, CatalogSort::Artist, limit).await,
            ["Charlie", "alpha", "Bravo"]
        );
    }
}

#[tokio::test]
async fn tracks_without_a_sort_keep_title_order_and_reject_a_year_sort() {
    let (_dir, hito, actor) = added_in_two_scans().await;
    let unsorted: Vec<_> = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|item| item.title)
        .collect();
    assert_eq!(unsorted, ["alpha", "Bravo", "Charlie"]);
    let error = hito
        .catalog
        .recordings(
            &actor,
            Search {
                sort: Some(CatalogSort::Year),
                ..Search::default()
            },
        )
        .await
        .err()
        .unwrap();
    assert!(matches!(error, Error::Invalid(_)));
}

#[tokio::test]
async fn albums_sort_by_recently_added_title_artist_or_year_and_page_without_gaps() {
    let (_dir, hito, actor) = added_in_two_scans().await;
    for limit in [1, 2, 200] {
        let recent = album_titles(&hito, &actor, CatalogSort::Recent, limit).await;
        assert_eq!(recent[0], "Undated album");
        assert_eq!(
            album_titles(&hito, &actor, CatalogSort::Title, limit).await,
            ["New album", "Old album", "Undated album"]
        );
        assert_eq!(
            album_titles(&hito, &actor, CatalogSort::Artist, limit).await,
            ["Undated album", "New album", "Old album"]
        );
        assert_eq!(
            album_titles(&hito, &actor, CatalogSort::Year, limit).await,
            ["New album", "Old album", "Undated album"]
        );
    }
}

#[tokio::test]
async fn albums_report_the_tagged_year_and_none_when_untagged() {
    let (_dir, hito, actor) = added_in_two_scans().await;
    let years: Vec<_> = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|album| (album.title, album.year))
        .collect();
    assert_eq!(
        years,
        [
            ("New album".to_owned(), Some(2021)),
            ("Old album".to_owned(), Some(1999)),
            ("Undated album".to_owned(), None),
        ]
    );
}

#[tokio::test]
async fn artists_sort_by_recently_added_or_name_and_reject_a_year_sort() {
    let (_dir, hito, actor) = added_in_two_scans().await;
    for limit in [1, 2, 200] {
        let recent = artist_names(&hito, &actor, CatalogSort::Recent, limit).await;
        assert_eq!(recent[0], "Xi");
        assert_eq!(
            artist_names(&hito, &actor, CatalogSort::Title, limit).await,
            ["Xi", "Yan", "Zed"]
        );
    }
    let error = hito
        .catalog
        .artists(
            &actor,
            Browse {
                sort: Some(CatalogSort::Year),
                ..Browse::default()
            },
        )
        .await
        .err()
        .unwrap();
    assert!(matches!(error, Error::Invalid(_)));
}

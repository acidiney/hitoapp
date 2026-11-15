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

fn write_tone(music: &Path, title: &str, album: Option<&str>, samples: u32, audio: i16) {
    let file = music.join(format!("{title}.wav"));
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8000u32.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(samples * 2).to_le_bytes());
    for index in 0..samples {
        bytes.extend_from_slice(&((index as i16 % 64) + audio).to_le_bytes());
    }
    std::fs::write(&file, bytes).unwrap();
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist("Owned band".into());
    if let Some(album) = album {
        tag.set_album(album.into());
        tag.insert_text(ItemKey::AlbumArtist, "Owned band".into());
    }
    tag.save_to_path(file, WriteOptions::default()).unwrap();
}

async fn timed_library() -> (tempfile::TempDir, Services, Actor) {
    let (dir, hito, actor, library, music) = library("sort-timed-host").await;
    write_tone(&music, "delta", Some("beta album"), 24000, 1);
    write_tone(&music, "Echo", Some("Alpha album"), 8000, 2);
    write_tone(&music, "foxtrot", None, 16000, 3);
    write_tone(&music, "Golf", Some("Alpha album"), 16000, 4);
    scan(&hito, &actor, &library).await;
    (dir, hito, actor)
}

async fn sorted_titles(
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
        assert_eq!(page.total, 4);
        if page.items.is_empty() {
            return titles;
        }
        offset += page.items.len() as i64;
        titles.extend(page.items.into_iter().map(|item| item.title));
    }
}

#[tokio::test]
async fn tracks_sort_by_album_with_missing_albums_last_and_by_duration() {
    let (_dir, hito, actor) = timed_library().await;
    for limit in [1, 3, 200] {
        assert_eq!(
            sorted_titles(&hito, &actor, CatalogSort::Album, limit).await,
            ["Echo", "Golf", "delta", "foxtrot"]
        );
        assert_eq!(
            sorted_titles(&hito, &actor, CatalogSort::Duration, limit).await,
            ["Echo", "foxtrot", "Golf", "delta"]
        );
    }
}

#[tokio::test]
async fn albums_and_artists_reject_track_only_sorts() {
    let (_dir, hito, actor) = added_in_two_scans().await;
    for sort in [CatalogSort::Album, CatalogSort::Duration] {
        let albums = hito
            .catalog
            .releases(
                &actor,
                Browse {
                    sort: Some(sort),
                    ..Browse::default()
                },
            )
            .await
            .err()
            .unwrap();
        assert!(matches!(albums, Error::Invalid(_)));
        let artists = hito
            .catalog
            .artists(
                &actor,
                Browse {
                    sort: Some(sort),
                    ..Browse::default()
                },
            )
            .await
            .err()
            .unwrap();
        assert!(matches!(artists, Error::Invalid(_)));
    }
}

use crate::test_support::{hang_guard::hang_guard, support, work_meter::WorkMeter};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};

const CAP: i64 = 20;
const SMALL: i16 = 40;
const LARGE: i16 = 300;
const FIXED_WORK: u64 = 30_000;

#[tokio::test]
async fn genre_track_continuation_reaches_past_the_bounded_count() {
    use std::collections::HashSet;
    use std::time::Duration;

    const TRACKS: usize = 1_001;
    const PAGE_SIZE: i64 = 200;
    const HOST: &str = "genre-page-over-1000";

    let (_directory, hito, actor, library, music) = support::library(HOST).await;
    let folder_id = hang_guard("genre continuation fixture", async {
        let seed = music.join("folder-seed.wav");
        support::write_wav(&seed, 17);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Folder seed");
        tag.set_artist("Owned artist".into());
        tag.set_genre("Jazz".into());
        tag.save_to_path(&seed, Default::default()).unwrap();
        support::scan(&hito, &actor, &library).await;

        let folder_id: String = sqlx::query_scalar(
            "SELECT id FROM music_folders WHERE library_id = ? AND host_device = ? AND root_path = ? AND connected = 1 AND bound = 1",
        )
        .bind(&library)
        .bind(HOST)
        .bind(music.to_string_lossy().as_ref())
        .fetch_one(&hito.catalog.store.pool)
        .await
        .unwrap();

        for index in 1..=TRACKS {
            support::write_wav(&music.join(format!("page-{index:04}.wav")), index as i16);
        }

        let mut tx = hito.catalog.store.pool.begin().await.unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < ?2)
             INSERT INTO recordings(id, library_id, title, artist, duration_ms, initial_file_checksum)
             SELECT printf('00000000-0000-4000-8000-%012x', i), ?1,
                    printf('Tied track %03d', (i - 1) / 10), 'Owned artist', 1000,
                    'genre-page-checksum-' || i
             FROM n",
        )
        .bind(&library)
        .bind(TRACKS as i64)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
             INSERT INTO sources(id, recording_id, provider, provider_id, device_id, ready)
             SELECT 'genre-page-source-' || i, printf('00000000-0000-4000-8000-%012x', i),
                    'local', 'genre-page-provider-' || i, ?2, 1
             FROM n",
        )
        .bind(TRACKS as i64)
        .bind(HOST)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
             INSERT INTO local_files(source_id, library_id, folder_id, relative_path, path, scan_root, size_bytes)
             SELECT 'genre-page-source-' || i, ?2, ?3,
                    json_quote('page-' || printf('%04d', i) || '.wav'),
                    ?4 || '/page-' || printf('%04d', i) || '.wav', ?4, 16044
             FROM n",
        )
        .bind(TRACKS as i64)
        .bind(&library)
        .bind(&folder_id)
        .bind(music.to_string_lossy().as_ref())
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
             INSERT INTO source_genres(source_id, genre_key, label)
             SELECT 'genre-page-source-' || i, 'fado', 'Fado' FROM n",
        )
        .bind(TRACKS as i64)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        folder_id
    })
    .await;

    let fado_count = tokio::time::timeout(
        Duration::from_secs(5),
        hito.catalog.genres_within(&actor, 18),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .and_then(|genres| {
        genres
            .into_iter()
            .find(|genre| genre.key == "fado")
            .map(|genre| genre.track_count)
    });

    let mut expected: Vec<(String, String)> = (1..=TRACKS)
        .map(|index| {
            (
                format!("Tied track {:03}", (index - 1) / 10),
                format!("00000000-0000-4000-8000-{index:012x}"),
            )
        })
        .collect();
    expected.sort_by(|(left_title, left_id), (right_title, right_id)| {
        left_title
            .to_lowercase()
            .cmp(&right_title.to_lowercase())
            .then_with(|| left_id.cmp(right_id))
    });

    let mut pages = Vec::new();
    let mut cursors = HashSet::new();
    let mut after: Option<String> = None;
    let mut page_failure = None;
    let mut repeated_cursor = false;
    for page_number in 0..6 {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            hito.catalog
                .genre_tracks(&actor, "Fado", Some(PAGE_SIZE), after.as_deref()),
        )
        .await;
        let Ok(Ok(page)) = result else {
            page_failure = Some(page_number);
            break;
        };
        match page.next_after {
            Some(cursor) => {
                repeated_cursor |= !cursors.insert(cursor.clone());
                after = Some(cursor);
            }
            None => {
                after = None;
            }
        }
        pages.push(page);
        if after.is_none() {
            break;
        }
    }

    let replication_closed = hito.catalog_replication.close().await;
    hito.catalog.store.pool.close().await;
    drop(hito);

    assert!(replication_closed.is_ok());
    assert_eq!(fado_count, Some(1_000), "the tile count is capped");
    assert_eq!(page_failure, None, "every page request succeeds");
    assert!(!repeated_cursor, "continuation cursors do not repeat");
    assert_eq!(pages.len(), 6);
    let mut actual = Vec::with_capacity(TRACKS);
    for (page_number, page) in pages.iter().enumerate() {
        let start = page_number * PAGE_SIZE as usize;
        let end = (start + PAGE_SIZE as usize).min(TRACKS);
        let page_ids: Vec<_> = page
            .items
            .iter()
            .map(|recording| recording.id.as_str())
            .collect();
        let expected_ids: Vec<_> = expected[start..end]
            .iter()
            .map(|(_, id)| id.as_str())
            .collect();
        assert_eq!(
            page_ids, expected_ids,
            "page {page_number} preserves title ties and ID order"
        );
        actual.extend(page.items.iter().map(|recording| recording.id.as_str()));
        assert_eq!(page.next_after.is_none(), page_number == 5);
    }
    assert_eq!(actual.len(), TRACKS);
    assert_eq!(actual.iter().collect::<HashSet<_>>().len(), TRACKS);
}

#[tokio::test]
async fn tile_work_stops_growing_once_a_genre_reaches_its_count_cap() {
    let (_directory, hito, actor, library, music) = support::library("genre-work").await;
    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let mut measured = Vec::new();
    let mut written = 0;
    for total in [SMALL, LARGE] {
        for index in written..total {
            let path = music.join(format!("{index}.wav"));
            support::write_wav(&path, index + 1);
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title(format!("Song {index}"));
            tag.set_artist("Performer".into());
            tag.set_album(format!("Album {index}"));
            tag.set_genre("Fado".into());
            tag.save_to_path(&path, Default::default()).unwrap();
        }
        written = total;
        support::scan(&hito, &actor, &library).await;
        let (tiles, work) = meter
            .measure(async { hito.catalog.genres_within(&actor, CAP).await.unwrap() })
            .await;
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].track_count, CAP, "the count stops at the cap");
        measured.push(work);
    }
    meter.remove(&hito.catalog.store.pool).await;
    eprintln!(
        "Genre tiles VM_steps small={} large={}",
        measured[0], measured[1]
    );
    let budget = measured[0] + FIXED_WORK;
    assert!(
        measured[1] < budget,
        "Tiles used {} VM steps with {LARGE} tagged tracks after {} with {SMALL}; budget {budget}",
        measured[1],
        measured[0]
    );
}

const ARTIST: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

#[tokio::test]
async fn musicbrainz_tile_work_stops_growing_once_a_genre_reaches_its_count_cap() {
    use crate::test_support::musicbrainz::{FakeMusicBrainz, Reply};
    use lofty::prelude::ItemKey;

    let fake = FakeMusicBrainz::start().await;
    fake.genres_of(
        "artist",
        ARTIST,
        Reply::Genres(vec![("Fado".into(), 3), ("Folk".into(), 1)]),
    );
    let (_directory, hito, actor, library, music) =
        support::library_with_musicbrainz("mb-genre-work", Some(fake.endpoint.clone())).await;
    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let mut measured = Vec::new();
    let mut written = 0;
    for total in [SMALL, LARGE] {
        for index in written..total {
            let path = music.join(format!("{index}.wav"));
            support::write_wav(&path, index + 1);
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title(format!("Song {index}"));
            tag.set_artist("Performer".into());
            tag.set_album(format!("Album {index}"));
            tag.insert_text(ItemKey::MusicBrainzArtistId, ARTIST.into());
            tag.save_to_path(&path, Default::default()).unwrap();
        }
        written = total;
        support::scan(&hito, &actor, &library).await;
        while hito
            .artist_catalog
            .run_next_automatic_matching_until(std::future::pending())
            .await
            .unwrap()
        {}
        let (tiles, work) = meter
            .measure(async { hito.catalog.genres_within(&actor, CAP).await.unwrap() })
            .await;
        assert_eq!(tiles.len(), 2);
        assert!(tiles.iter().all(|tile| tile.track_count == CAP));
        measured.push(work);
    }
    meter.remove(&hito.catalog.store.pool).await;
    eprintln!(
        "MusicBrainz genre tiles VM_steps small={} large={}",
        measured[0], measured[1]
    );
    let budget = measured[0] + FIXED_WORK;
    assert!(
        measured[1] < budget,
        "Tiles used {} VM steps with {LARGE} tracks after {} with {SMALL}; budget {budget}",
        measured[1],
        measured[0]
    );
}

#[tokio::test]
async fn rows_that_name_one_recording_twice_do_not_use_up_the_count_cap() {
    use crate::test_support::musicbrainz::{FakeMusicBrainz, Reply};
    use lofty::prelude::ItemKey;

    const GROUP: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    let fake = FakeMusicBrainz::start().await;
    fake.genres_of(
        "release-group",
        GROUP,
        Reply::Genres(vec![("Ambient".into(), 2)]),
    );
    let (directory, hito, actor, library, music) =
        support::library_with_musicbrainz("mb-genre-dups", Some(fake.endpoint.clone())).await;
    for index in 0..30i16 {
        let title = format!("Song {index}");
        let (path, mut tag) = if index < 12 {
            let source = directory.path().join(format!("{index}.wav"));
            support::write_wav(&source, index + 1);
            let path = music.join(format!("{index}.flac"));
            let status = std::process::Command::new("ffmpeg")
                .args(["-nostdin", "-v", "error", "-y", "-i"])
                .arg(&source)
                .args(["-c:a", "flac"])
                .arg(&path)
                .status()
                .unwrap();
            assert!(status.success());
            let mut tag = Tag::new(TagType::VorbisComments);
            let recording = format!("dddddddd-dddd-4ddd-8ddd-{index:012x}");
            tag.insert_text(ItemKey::MusicBrainzRecordingId, recording.clone());
            fake.genres_of(
                "recording",
                &recording,
                Reply::Genres(vec![("Ambient".into(), 1)]),
            );
            (path, tag)
        } else {
            let path = music.join(format!("{index}.wav"));
            support::write_wav(&path, index + 1);
            (path, Tag::new(TagType::Id3v2))
        };
        tag.set_title(title);
        tag.set_artist("Performer".into());
        tag.insert_text(ItemKey::MusicBrainzReleaseGroupId, GROUP.into());
        tag.save_to_path(&path, Default::default()).unwrap();
    }
    support::scan(&hito, &actor, &library).await;
    while hito
        .artist_catalog
        .run_next_automatic_matching_until(std::future::pending())
        .await
        .unwrap()
    {}
    let tiles = hito.catalog.genres_within(&actor, CAP).await.unwrap();
    assert_eq!(tiles.len(), 1);
    assert_eq!(
        tiles[0].track_count, CAP,
        "30 recordings count up to the cap"
    );
}

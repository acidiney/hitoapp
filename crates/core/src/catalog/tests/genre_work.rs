use crate::test_support::{support, work_meter::WorkMeter};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};

const CAP: i64 = 20;
const SMALL: i16 = 40;
const LARGE: i16 = 300;
const FIXED_WORK: u64 = 30_000;

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

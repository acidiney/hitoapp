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

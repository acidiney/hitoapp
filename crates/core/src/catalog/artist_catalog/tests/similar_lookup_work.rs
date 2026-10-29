use super::super::identity::groups::chosen_groups_by_musicbrainz_id;
use crate::{
    MatchArtistsFromTags,
    test_support::{support, work_meter::WorkMeter},
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use uuid::Uuid;

const SMALL: i16 = 20;
const LARGE: i16 = 300;
const FIXED_WORK: u64 = 30_000;
const WORK_PER_CHOSEN_ARTIST: u64 = 40;

fn mbid(index: i16) -> String {
    format!("{:08x}-0000-4000-8000-000000000000", index as u32 + 1)
}

#[tokio::test]
async fn exact_identity_lookup_work_does_not_grow_with_the_account_s_chosen_artists() {
    let (_directory, hito, actor, library, music) = support::library("similar-lookup-work").await;
    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let mut measured = Vec::new();
    let mut written = 0;
    for total in [SMALL, LARGE] {
        for index in written..total {
            let path = music.join(format!("{index}.wav"));
            support::write_wav(&path, index + 1);
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title(format!("Song {index}"));
            tag.set_artist(format!("Performer {index}"));
            tag.set_album(format!("Album {index}"));
            tag.insert_text(ItemKey::MusicBrainzArtistId, mbid(index));
            tag.save_to_path(&path, Default::default()).unwrap();
        }
        written = total;
        support::scan(&hito, &actor, &library).await;
        let mut after = None;
        loop {
            let receipt = hito
                .catalog
                .match_artists_from_tags(
                    &actor,
                    MatchArtistsFromTags {
                        request_id: Uuid::new_v4(),
                        after: after.take(),
                        limit: None,
                    },
                )
                .await
                .unwrap();
            if receipt.next_after.is_none() {
                break;
            }
            after = receipt.next_after;
        }
        let ids = vec![mbid(0), mbid(total - 1), Uuid::new_v4().to_string()];
        let (found, work) = meter
            .measure(async {
                let mut connection = hito.catalog.store.pool.acquire().await.unwrap();
                chosen_groups_by_musicbrainz_id(&mut connection, &actor, &ids)
                    .await
                    .unwrap()
            })
            .await;
        assert_eq!(found.len(), 2, "only exact chosen identities are found");
        measured.push(work);
    }
    meter.remove(&hito.catalog.store.pool).await;
    eprintln!(
        "Similar lookup VM_steps small={} large={}",
        measured[0], measured[1]
    );
    let budget = measured[0] + FIXED_WORK + (LARGE - SMALL) as u64 * WORK_PER_CHOSEN_ARTIST / 20;
    assert!(
        measured[1] < budget,
        "Lookup used {} VM steps with {LARGE} chosen artists after {} with {SMALL}; budget {budget}",
        measured[1],
        measured[0]
    );
}

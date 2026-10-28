use crate::{
    MatchArtistsFromTags,
    test_support::{support, work_meter::WorkMeter},
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use uuid::Uuid;

const AMBIGUOUS: i16 = 30;
const UNTAGGED: i16 = 30;
const BUDGET: u64 = 40_000;

fn write(path: &std::path::Path, offset: i16, ambiguous: bool) {
    support::write_wav(path, offset);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(format!("Song {offset}"));
    tag.set_artist("Crowd".into());
    if ambiguous {
        tag.insert_text(ItemKey::MusicBrainzArtistId, "not-an-identifier".into());
    }
    tag.save_to_path(path, Default::default()).unwrap();
}

#[tokio::test]
async fn a_second_pass_over_ambiguous_and_untagged_recordings_does_no_rework() {
    let (_directory, hito, actor, library, music) = support::library("artist-match-work").await;
    for offset in 0..AMBIGUOUS + UNTAGGED {
        write(
            &music.join(format!("{offset}.wav")),
            offset + 1,
            offset < AMBIGUOUS,
        );
    }
    support::scan(&hito, &actor, &library).await;
    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let mut run = async || {
        let (receipt, work) = meter
            .measure(hito.catalog.match_artists_from_tags(
                &actor,
                MatchArtistsFromTags {
                    request_id: Uuid::new_v4(),
                    after: None,
                    limit: None,
                },
            ))
            .await;
        (receipt.unwrap(), work)
    };
    let (first, first_work) = run().await;
    let (second, second_work) = run().await;
    meter.remove(&hito.catalog.store.pool).await;
    eprintln!("Artist matching VM_steps first={first_work} second={second_work}");
    assert_eq!(
        (first.matched, first.skipped_ambiguous),
        (0, AMBIGUOUS as i64)
    );
    assert_eq!(
        (
            second.matched,
            second.skipped_ambiguous,
            second.skipped_declined
        ),
        (0, 0, 0)
    );
    let checks: i64 = sqlx::query_scalar("SELECT count(*) FROM artist_match_checks")
        .fetch_one(&hito.catalog.store.pool)
        .await
        .unwrap();
    assert_eq!(checks, AMBIGUOUS as i64);
    assert!(
        second_work <= first_work && first_work < BUDGET,
        "Matching used {first_work} then {second_work} VM steps for {} recordings against a budget of {BUDGET}",
        AMBIGUOUS + UNTAGGED
    );
}

const GROUPS: i16 = 20;
const SUGGESTION_BUDGET: u64 = 150_000;

#[tokio::test]
async fn suggestions_for_a_credit_with_many_namesake_groups_stay_in_budget() {
    let (_directory, hito, actor, library, music) = support::library("artist-suggest-work").await;
    for offset in 0..GROUPS {
        support::write_wav(&music.join(format!("t{offset}.wav")), offset + 1);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(format!("Tagged {offset}"));
        tag.set_artist("Crowd".into());
        tag.set_album(format!("Album {offset}"));
        tag.insert_text(
            ItemKey::MusicBrainzArtistId,
            format!("00000000-0000-4000-8000-{:012}", offset + 1),
        );
        tag.save_to_path(music.join(format!("t{offset}.wav")), Default::default())
            .unwrap();
        write(&music.join(format!("u{offset}.wav")), 100 + offset, false);
    }
    support::scan(&hito, &actor, &library).await;
    let matched = hito
        .catalog
        .match_artists_from_tags(
            &actor,
            MatchArtistsFromTags {
                request_id: Uuid::new_v4(),
                after: None,
                limit: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(matched.matched, GROUPS as i64);
    let credit = hito
        .catalog
        .artists(&actor, Default::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let (found, work) = meter
        .measure(hito.catalog.artist_match_suggestions(&actor, &credit))
        .await;
    meter.remove(&hito.catalog.store.pool).await;
    eprintln!("Artist suggestions VM_steps groups={GROUPS} work={work}");
    assert_eq!(found.unwrap().items.len(), GROUPS as usize);
    assert!(
        work < SUGGESTION_BUDGET,
        "Suggestions used {work} VM steps for {GROUPS} groups against {SUGGESTION_BUDGET}"
    );
}

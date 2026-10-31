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
    let run = async || {
        let (receipt, work) = meter
            .measure(hito.artist_catalog.match_artists_from_tags(
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
        .artist_catalog
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
        .measure(
            hito.artist_catalog
                .artist_match_suggestions(&actor, &credit),
        )
        .await;
    meter.remove(&hito.catalog.store.pool).await;
    eprintln!("Artist suggestions VM_steps groups={GROUPS} work={work}");
    assert_eq!(found.unwrap().items.len(), GROUPS as usize);
    assert!(
        work < SUGGESTION_BUDGET,
        "Suggestions used {work} VM steps for {GROUPS} groups against {SUGGESTION_BUDGET}"
    );
}

const QUEUE_CREDITS: usize = 60;
const QUEUE_TRACKS: usize = 40;
const QUEUE_BUDGET: u64 = 300_000;

#[tokio::test]
async fn the_review_queue_reads_a_large_library_in_one_pass_over_its_checks() {
    let (_directory, hito, actor, library, _music) = support::library("artist-queue-work").await;
    let pool = &hito.catalog.store.pool;
    let mut connection = pool.acquire().await.unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=OFF")
        .execute(&mut *connection)
        .await
        .unwrap();
    let mut tx = sqlx::Connection::begin(&mut *connection).await.unwrap();
    for credit in 0..QUEUE_CREDITS {
        let artist = format!("credit-{credit:03}");
        sqlx::query("INSERT INTO account_artists(user_id,id,library_id,name) VALUES(?,?,?,?)")
            .bind(actor.id())
            .bind(&artist)
            .bind(&library)
            .bind(format!("Credit {credit:03}"))
            .execute(&mut *tx)
            .await
            .unwrap();
        for provider in ["a", "b", "c"] {
            sqlx::query("INSERT INTO artist_name_candidates(user_id,artist_id,provider_artist_id,position,name,corroborated_titles) VALUES(?,?,?,0,'Name',0)")
                .bind(actor.id())
                .bind(&artist)
                .bind(format!("{provider}-{credit}"))
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        for track in 0..QUEUE_TRACKS {
            let recording = format!("queue-{credit:03}-{track:03}");
            sqlx::query("INSERT INTO recordings(id,library_id,title,artist,metadata_observation_id) VALUES(?1,?2,?1,'Seed','observation')")
                .bind(&recording)
                .bind(&library)
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("INSERT INTO account_recording_credits(user_id,recording_id,artist_id) VALUES(?,?,?)")
                .bind(actor.id())
                .bind(&recording)
                .bind(&artist)
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("INSERT INTO artist_name_checks(user_id,recording_id,observation_id,rule) VALUES(?,?,'observation',?)")
                .bind(actor.id())
                .bind(&recording)
                .bind(super::name_discography::NAME_RULE)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    tx.commit().await.unwrap();
    drop(connection);
    let meter = WorkMeter::install(pool).await;
    let (page, work) = meter
        .measure(hito.artist_catalog.artist_review_queue(
            &actor,
            crate::ArtistReviewQueueQuery {
                offset: Some(0),
                limit: Some(50),
            },
        ))
        .await;
    meter.remove(pool).await;
    let page = page.unwrap();
    eprintln!(
        "Artist review queue VM_steps credits={QUEUE_CREDITS} tracks={QUEUE_TRACKS} work={work}"
    );
    assert_eq!((page.total, page.items.len()), (QUEUE_CREDITS as i64, 50));
    assert!(page.items.iter().all(|item| item.candidates.len() == 3));
    assert!(
        work < QUEUE_BUDGET,
        "The queue used {work} VM steps for {} checked recordings against {QUEUE_BUDGET}",
        QUEUE_CREDITS * QUEUE_TRACKS
    );
}

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

async fn seed_review_credits(hito: &crate::Services, actor: &crate::Actor, library: &str) {
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
            .bind(library)
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
                .bind(library)
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
}

#[tokio::test]
async fn the_review_queue_reads_a_large_library_in_one_pass_over_its_checks() {
    let (_directory, hito, actor, library, _music) = support::library("artist-queue-work").await;
    seed_review_credits(&hito, &actor, &library).await;
    let pool = &hito.catalog.store.pool;
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

const OPEN_CREDITS: usize = 20;
const EDITED_CREDITS: usize = 10;
const CHOSEN_RECORDINGS: usize = 200;
const MATCHED_CREDITS: usize = 10;
const MIXED_BUDGET: u64 = 400_000;

async fn seed_open_and_edited_credits(hito: &crate::Services, actor: &crate::Actor, library: &str) {
    let pool = &hito.catalog.store.pool;
    let mut connection = pool.acquire().await.unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=OFF")
        .execute(&mut *connection)
        .await
        .unwrap();
    let mut tx = sqlx::Connection::begin(&mut *connection).await.unwrap();
    for credit in 0..OPEN_CREDITS + EDITED_CREDITS {
        let edited = credit >= OPEN_CREDITS;
        let artist = format!("open-{credit:03}");
        sqlx::query("INSERT INTO account_artists(user_id,id,library_id,name) VALUES(?,?,?,?)")
            .bind(actor.id())
            .bind(&artist)
            .bind(library)
            .bind(format!("Open {credit:03}"))
            .execute(&mut *tx)
            .await
            .unwrap();
        for track in 0..QUEUE_TRACKS {
            let recording = format!("open-{credit:03}-{track:03}");
            sqlx::query("INSERT INTO recordings(id,library_id,title,artist,metadata_observation_id) VALUES(?1,?2,?1,'Seed','observation')")
                .bind(&recording)
                .bind(library)
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
            if edited {
                sqlx::query("INSERT INTO personal_recording_metadata(user_id,recording_id,title,artist,override_artist,artist_operation_id) VALUES(?1,?2,?2,'Edited','Edited','operation')")
                    .bind(actor.id())
                    .bind(&recording)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            } else {
                sqlx::query("INSERT INTO artist_name_checks(user_id,recording_id,observation_id,rule) VALUES(?,?,'observation',?)")
                    .bind(actor.id())
                    .bind(&recording)
                    .bind(super::name_discography::NAME_RULE)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
        }
    }
    sqlx::query("INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,provider_artist_id) VALUES('group',?,'builtin_musicbrainz','mbid')")
        .bind(actor.id())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql("DROP TRIGGER recording_artist_association_scope")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,observation_id,role,provider_kind,provider_artist_id,evidence) VALUES('association',?,?,'matched-000-000','matched-000','observation','recording_artist','builtin_musicbrainz','mbid','user_confirmed')")
        .bind(actor.id())
        .bind(library)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO artist_association_name_matches(association_id,evidence_kind,receipt_order,rule) VALUES('association','name_discography',1,5)")
        .execute(&mut *tx)
        .await
        .unwrap();
    for credit in 0..MATCHED_CREDITS {
        let artist = format!("matched-{credit:03}");
        sqlx::query("INSERT INTO account_artists(user_id,id,library_id,name) VALUES(?,?,?,?)")
            .bind(actor.id())
            .bind(&artist)
            .bind(library)
            .bind(format!("Matched {credit:03}"))
            .execute(&mut *tx)
            .await
            .unwrap();
        for track in 0..QUEUE_TRACKS {
            let recording = format!("matched-{credit:03}-{track:03}");
            sqlx::query("INSERT INTO recordings(id,library_id,title,artist,metadata_observation_id) VALUES(?1,?2,?1,'Seed','observation')")
                .bind(&recording)
                .bind(library)
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
            if track % 2 == 0 {
                sqlx::query("INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,observation_id,selection_order) VALUES(?1,?2,1,'association','group',?3,'observation',1)")
                    .bind(actor.id())
                    .bind(&recording)
                    .bind(&artist)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            } else {
                sqlx::query("INSERT INTO artist_name_checks(user_id,recording_id,observation_id,rule) VALUES(?,?,'observation',?)")
                    .bind(actor.id())
                    .bind(&recording)
                    .bind(super::name_discography::NAME_RULE)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
        }
    }
    for index in 0..CHOSEN_RECORDINGS {
        let recording = format!(
            "queue-{:03}-{:03}",
            index / QUEUE_TRACKS,
            index % QUEUE_TRACKS
        );
        sqlx::query("INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,observation_id,selection_order) VALUES(?1,?2,1,'association','group',?3,'observation',1)")
            .bind(actor.id())
            .bind(&recording)
            .bind(format!("credit-{:03}", index / QUEUE_TRACKS))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    drop(connection);
}

#[tokio::test]
async fn the_review_queue_with_open_and_edited_credits_stays_in_budget() {
    let (_directory, hito, actor, library, _music) =
        support::library("artist-queue-mixed-work").await;
    seed_review_credits(&hito, &actor, &library).await;
    seed_open_and_edited_credits(&hito, &actor, &library).await;
    let pool = &hito.catalog.store.pool;
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
        "Artist review queue mixed VM_steps credits={QUEUE_CREDITS}+{OPEN_CREDITS}+{EDITED_CREDITS} tracks={QUEUE_TRACKS} work={work}"
    );
    assert_eq!((page.total, page.items.len()), (85, 50));
    let tail = hito
        .artist_catalog
        .artist_review_queue(
            &actor,
            crate::ArtistReviewQueueQuery {
                offset: Some(50),
                limit: Some(50),
            },
        )
        .await
        .unwrap();
    assert_eq!(tail.total, 85);
    assert_eq!(tail.items.len(), 35);
    assert_eq!(
        tail.items
            .iter()
            .filter(|item| item.candidates.iter().any(|offer| offer.origin
                == crate::ArtistReviewOrigin::MatchedOnOtherTracks
                && offer.corroborated_titles == QUEUE_TRACKS as i64 / 2))
            .count(),
        MATCHED_CREDITS
    );
    assert!(
        tail.items
            .iter()
            .skip(15)
            .all(|item| item.needs_search && item.search_recording_id.is_some())
    );
    assert!(
        work < MIXED_BUDGET,
        "The queue used {work} VM steps against {MIXED_BUDGET}"
    );
}

#[tokio::test]
async fn the_dismissed_read_of_a_large_library_stays_in_budget() {
    let (_directory, hito, actor, library, _music) =
        support::library("artist-dismissed-work").await;
    seed_review_credits(&hito, &actor, &library).await;
    let pool = &hito.catalog.store.pool;
    sqlx::query("INSERT INTO artist_review_dismissals(user_id,artist_id,provider_artist_id) SELECT user_id,artist_id,provider_artist_id FROM artist_name_candidates")
        .execute(pool)
        .await
        .unwrap();
    let meter = WorkMeter::install(pool).await;
    let (page, work) = meter
        .measure(hito.artist_catalog.dismissed_artist_reviews(
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
        "Artist dismissed reviews VM_steps credits={QUEUE_CREDITS} tracks={QUEUE_TRACKS} work={work}"
    );
    assert_eq!((page.total, page.items.len()), (QUEUE_CREDITS as i64, 50));
    assert!(page.items.iter().all(|item| item.candidates.len() == 3));
    assert!(
        work < QUEUE_BUDGET,
        "The dismissed read used {work} VM steps for {} checked recordings against {QUEUE_BUDGET}",
        QUEUE_CREDITS * QUEUE_TRACKS
    );
}

const INTERMEDIATE_RESULTS: [&str; 6] = [
    "base",
    "flags",
    "undecided",
    "edited",
    "chosen_names",
    "verdicts",
];

fn plan_lines(rows: Vec<(i64, i64, i64, String)>) -> Vec<String> {
    rows.into_iter().map(|row| row.3).collect()
}

fn assert_no_table_scan(plan: &[String]) {
    for line in plan.iter().filter(|line| line.starts_with("SCAN")) {
        let target = line.split_whitespace().nth(1).unwrap_or_default();
        assert!(
            INTERMEDIATE_RESULTS.contains(&target) || target.starts_with("(subquery"),
            "{target} is scanned: {line}\n{}",
            plan.join("\n")
        );
    }
}

fn assert_searched(plan: &[String], expected: &[&str]) {
    for part in expected {
        assert!(
            plan.iter()
                .any(|line| line.starts_with("SEARCH") && line.contains(part)),
            "No line searches {part}\n{}",
            plan.join("\n")
        );
    }
}

const MATCHED_SEARCHES: [&str; 7] = [
    "credit USING COVERING INDEX account_recording_artists (user_id=? AND artist_id=?)",
    "choice USING INDEX sqlite_autoindex_recording_artist_identity_choices_1 (user_id=? AND recording_id=?)",
    "g USING INDEX sqlite_autoindex_account_artist_identity_groups_2 (user_id=? AND id=?)",
    "m USING COVERING INDEX sqlite_autoindex_library_members_1",
    "p USING INDEX sqlite_autoindex_personal_recording_metadata_1 (user_id=? AND recording_id=?)",
    "assoc USING INDEX sqlite_autoindex_recording_artist_associations_1 (id=?)",
    "d USING COVERING INDEX sqlite_autoindex_artist_review_dismissals_1",
];

#[tokio::test]
async fn the_review_queue_plan_scans_only_its_own_intermediate_results() {
    use super::review_queue::{Listing, matched_counts, queued};
    let (_directory, hito, actor, library, _music) = support::library("artist-queue-explain").await;
    seed_review_credits(&hito, &actor, &library).await;
    seed_open_and_edited_credits(&hito, &actor, &library).await;
    let pool = &hito.catalog.store.pool;
    for listing in [Listing::Offered, Listing::Dismissed] {
        let sql = format!(
            "EXPLAIN QUERY PLAN SELECT id,name,tracks,stored,chosen,matched,count(*) OVER () FROM ({}) ORDER BY (stored+chosen+matched)=0,tracks DESC,name,id LIMIT ?2 OFFSET ?3",
            queued(listing)
        );
        let plan = plan_lines(
            sqlx::query_as(&sql)
                .bind(actor.id())
                .bind(50_i64)
                .bind(0_i64)
                .fetch_all(pool)
                .await
                .unwrap(),
        );
        assert_no_table_scan(&plan);
        assert_searched(&plan, &MATCHED_SEARCHES);
    }
    let page = format!(
        "EXPLAIN QUERY PLAN {}",
        matched_counts("AND credit.artist_id IN (SELECT value FROM json_each(?2))")
    );
    let plan = plan_lines(
        sqlx::query_as(&page)
            .bind(actor.id())
            .bind("[\"credit-000\"]")
            .fetch_all(pool)
            .await
            .unwrap(),
    );
    assert_searched(&plan, &MATCHED_SEARCHES[..6]);
    assert!(
        plan.iter()
            .filter(|line| line.starts_with("SCAN"))
            .all(|line| line.contains("json_each")
                || line.contains("value")
                || line.contains("(subquery")),
        "{}",
        plan.join("\n")
    );
}

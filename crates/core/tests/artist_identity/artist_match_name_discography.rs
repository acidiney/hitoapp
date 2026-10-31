#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{
    ArtistAssociationEvidence, MetadataProvider, ResetRecordingArtistIdentity,
    UpdateMetadataSettings,
};
use library::{CREDIT, NAMESAKE, NOVA, NameLibrary, TRIBUTE, nova_discography};
use std::time::Duration;
use uuid::Uuid;

const SEARCH: &str = "/ws/2/artist?query=%22Nova+Quartet%22&fmt=json&limit=100&offset=0";

/// The recording searches made about one artist.
fn probes_of(library: &NameLibrary, artist: &str) -> Vec<String> {
    let prefix = format!("/ws/2/recording?query=arid%3A{artist}+AND");
    library
        .musicbrainz
        .requests()
        .into_iter()
        .filter(|request| request.starts_with(&prefix))
        .collect()
}

fn candidates(entries: &[(&str, &str)]) -> Reply {
    Reply::Candidates(
        entries
            .iter()
            .map(|(id, name)| ((*id).into(), (*name).into()))
            .collect(),
    )
}

/// Three of the four library tracks appear in the discography with a length inside two seconds.
fn corroborating() -> Reply {
    nova_discography(&[
        ("morning light", Some(202_900)),
        ("Second  Wind", Some(185_500)),
        ("HARBOR LIGHTS", Some(243_000)),
        ("Another Song", Some(100_000)),
    ])
}

#[tokio::test]
async fn one_exact_name_artist_whose_discography_corroborates_two_tracks_matches_them() {
    let library = NameLibrary::nova().await;
    library.musicbrainz.search(
        CREDIT,
        candidates(&[(NOVA, CREDIT), (TRIBUTE, "Nova Quartet Tribute")]),
    );
    library.musicbrainz.recordings_of(NOVA, corroborating());

    let receipt = library.run().await;
    assert_eq!(
        (
            receipt.matched,
            receipt.uncertain,
            receipt.declined,
            receipt.unavailable
        ),
        (3, 1, 0, 0)
    );
    let group = library.group_of(&library.recordings[0]).await.unwrap();
    for matched in &library.recordings[..3] {
        assert_eq!(library.group_of(matched).await.as_ref(), Some(&group));
        let linked = library.associations(matched).await;
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].provider, MetadataProvider::BuiltinMusicBrainz);
        assert_eq!(linked[0].provider_artist_id, NOVA);
        assert_eq!(
            linked[0].evidence,
            ArtistAssociationEvidence::MatchedNameDiscography
        );
    }
    assert!(
        library.group_of(&library.recordings[3]).await.is_none(),
        "A track the artist's recordings do not list stays undecided"
    );
    let requests = library.musicbrainz.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0], SEARCH);
    assert_eq!(
        probes_of(&library, NOVA).len(),
        1,
        "A name that differs from the credit is never asked about, and the four titles go in one search"
    );
    assert_eq!(library.rows("artist_association_name_matches").await, 3);
    assert_eq!(library.rows("artist_association_credit_matches").await, 0);
    assert_eq!(library.rows("artist_name_checks").await, 1);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn two_artists_sharing_the_name_that_both_corroborate_stay_uncertain() {
    let library = NameLibrary::nova().await;
    library.musicbrainz.search(
        CREDIT,
        candidates(&[(NOVA, CREDIT), (NAMESAKE, "nova  quartet")]),
    );
    library.musicbrainz.recordings_of(NOVA, corroborating());
    library.musicbrainz.recordings_of(NAMESAKE, corroborating());

    let receipt = library.run().await;
    assert_eq!(
        (receipt.matched, receipt.uncertain, receipt.unavailable),
        (0, 4, 0)
    );
    for recording in &library.recordings {
        assert!(library.group_of(recording).await.is_none());
    }
    assert_eq!(library.rows("artist_association_name_matches").await, 0);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_namesake_that_does_not_corroborate_does_not_block_the_one_that_does() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT), (NAMESAKE, CREDIT)]));
    library.musicbrainz.recordings_of(NOVA, corroborating());
    library
        .musicbrainz
        .recordings_of(NAMESAKE, nova_discography(&[("Unrelated", Some(1000))]));

    let receipt = library.run().await;
    assert_eq!(receipt.matched, 3);
    assert_eq!(
        library.associations(&library.recordings[0]).await[0].provider_artist_id,
        NOVA
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_namesake_whose_discography_was_not_fully_read_keeps_the_credit_uncertain() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT), (NAMESAKE, CREDIT)]));
    library.musicbrainz.recordings_of(NOVA, corroborating());
    let crowd: Vec<(String, Option<u64>)> = (0..45)
        .map(|index| (format!("Morning Light (version {index})"), Some(201_000)))
        .collect();
    library
        .musicbrainz
        .recordings_of(NAMESAKE, Reply::Discography(crowd));

    let receipt = library.run().await;
    assert_eq!((receipt.matched, receipt.uncertain), (0, 4));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn corroboration_below_the_threshold_is_not_a_match() {
    for discography in [
        nova_discography(&[("Morning Light", Some(201_000))]),
        nova_discography(&[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(190_000)),
        ]),
        nova_discography(&[("Morning Light", Some(201_000)), ("Second Wind", None)]),
        nova_discography(&[
            ("Morning Light", Some(201_000)),
            ("Second Windows", Some(187_500)),
        ]),
    ] {
        let library = NameLibrary::nova().await;
        library
            .musicbrainz
            .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
        library.musicbrainz.recordings_of(NOVA, discography);
        let receipt = library.run().await;
        assert_eq!((receipt.matched, receipt.uncertain), (0, 4));
        assert!(library.group_of(&library.recordings[0]).await.is_none());
        library.origin.shutdown().await;
    }
}

#[tokio::test]
async fn a_length_exactly_two_seconds_apart_corroborates() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[
            ("Morning Light", Some(203_000)),
            ("Second Wind", Some(185_500)),
        ]),
    );
    assert_eq!(library.run().await.matched, 2);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn no_exact_name_artist_and_an_unknown_credit_are_uncertain_and_not_asked_again() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(TRIBUTE, "Nova Quartet Tribute")]));
    let receipt = library.run().await;
    assert_eq!((receipt.matched, receipt.uncertain), (0, 4));
    assert_eq!(library.musicbrainz.requests(), vec![SEARCH.to_string()]);
    assert_eq!(library.rows("artist_name_checks").await, 4);
    let again = library.run().await;
    assert_eq!(
        (again.matched, again.uncertain, again.unavailable),
        (0, 0, 0)
    );
    assert_eq!(
        library.musicbrainz.requests().len(),
        1,
        "A settled answer is not asked again for an unchanged recording"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn an_outage_is_unavailable_and_retried_by_a_later_pass() {
    let library = NameLibrary::nova().await;
    library.musicbrainz.search(CREDIT, Reply::Unavailable);
    let first = library.run().await;
    assert_eq!(
        (first.matched, first.uncertain, first.unavailable),
        (0, 0, 4)
    );
    assert_eq!(library.rows("artist_name_checks").await, 0);

    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(NOVA, Reply::Unavailable);
    let second = library.run().await;
    assert_eq!((second.matched, second.unavailable), (0, 4));

    library.musicbrainz.recordings_of(NOVA, corroborating());
    let third = library.run().await;
    assert_eq!((third.matched, third.unavailable), (3, 0));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_pass_cancelled_while_the_provider_is_slow_writes_nothing_and_can_be_repeated() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(NOVA, corroborating());
    library.musicbrainz.delay_replies(Duration::from_secs(3));
    let cancelled = tokio::time::timeout(
        Duration::from_millis(400),
        library.request(Some("name:".into())),
    )
    .await;
    assert!(
        cancelled.is_err(),
        "The pass is still waiting for MusicBrainz"
    );
    assert_eq!(library.rows("artist_association_name_matches").await, 0);
    assert_eq!(library.rows("artist_name_checks").await, 0);
    library.musicbrainz.delay_replies(Duration::ZERO);
    assert_eq!(library.run().await.matched, 3);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn disabled_enrichment_and_the_silent_tag_pass_send_no_name() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(NOVA, corroborating());
    library.tag_pass().await;
    assert!(
        library.musicbrainz.requests().is_empty(),
        "Only the explicit provider pass may contact MusicBrainz"
    );
    let settings = library
        .hito
        .artist_catalog
        .metadata_settings(&library.actor)
        .await
        .unwrap();
    library
        .hito
        .artist_catalog
        .set_metadata_settings(
            &library.actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: false,
                providers: settings.providers.clone(),
            },
        )
        .await
        .unwrap();
    let receipt = library.run().await;
    assert_eq!(
        (receipt.matched, receipt.uncertain, receipt.unavailable),
        (0, 0, 0)
    );
    let named = library.request(Some("name:".into())).await.unwrap();
    assert_eq!(named.next_after, None);
    assert!(library.musicbrainz.requests().is_empty());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn personal_decisions_are_never_overridden_and_a_credit_with_one_undecided_track_asks_nothing()
 {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(NOVA, corroborating());
    let decided = &library.recordings[0];
    let choice = library
        .hito
        .artist_catalog
        .recording_artist_identity_choice(&library.actor, decided)
        .await
        .unwrap();
    let reset = ResetRecordingArtistIdentity {
        request_id: Uuid::new_v4(),
        expected_revision: choice.revision,
    };
    library
        .hito
        .artist_catalog
        .reset_recording_artist_identity(&library.actor, decided, reset)
        .await
        .unwrap();
    let receipt = library.run().await;
    assert_eq!(
        receipt.matched, 2,
        "The decided track is neither asked about nor matched; two of the other three are listed"
    );
    assert!(library.group_of(decided).await.is_none());

    let single = NameLibrary::prepare(&[(CREDIT, "Morning Light", 201_000)]).await;
    single
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    assert_eq!(single.run().await.matched, 0);
    assert!(
        single.musicbrainz.requests().is_empty(),
        "A single track can never be corroborated, so its name is not sent"
    );
    single.origin.shutdown().await;
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_request_that_matched_replays_exactly_without_asking_again() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(NOVA, corroborating());
    let request = hito_core::MatchArtistsFromProviderEvidence {
        request_id: Uuid::new_v4(),
        after: Some("name:".into()),
        limit: None,
    };
    let first = library
        .hito
        .artist_catalog
        .match_artists_from_provider_evidence(&library.actor, request.clone())
        .await
        .unwrap();
    assert_eq!(first.matched, 3);
    let asked = library.musicbrainz.requests().len();
    let replay = library
        .hito
        .artist_catalog
        .match_artists_from_provider_evidence(&library.actor, request)
        .await
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(library.musicbrainz.requests().len(), asked);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn one_request_performs_at_most_eight_lookups_and_the_walk_resumes_by_cursor() {
    let credits: Vec<String> = (0..6)
        .map(|index| format!("Credit Number {index}"))
        .collect();
    let mut entries = Vec::new();
    for credit in &credits {
        for (title, length) in [("First Song", 100_000), ("Second Song", 120_000)] {
            entries.push((credit.as_str(), title, length));
        }
    }
    let library = NameLibrary::prepare(&entries).await;
    for (index, credit) in credits.iter().enumerate() {
        let artists: Vec<(String, String)> = (0..3)
            .map(|namesake| {
                (
                    format!(
                        "0f700000-0000-4000-8000-{:012x}",
                        (index + 1) * 16 + namesake
                    ),
                    credit.clone(),
                )
            })
            .collect();
        for (id, _) in &artists {
            library
                .musicbrainz
                .recordings_of(id, nova_discography(&[("Elsewhere", Some(1))]));
        }
        library
            .musicbrainz
            .search(credit, Reply::Candidates(artists));
    }
    let mut after = Some("name:".to_string());
    let mut seen = 0;
    let mut batches = 0;
    while let Some(cursor) = after {
        let before = library.musicbrainz.requests().len();
        let receipt = library.request(Some(cursor)).await.unwrap();
        let used = library.musicbrainz.requests().len() - before;
        assert!(used <= 8, "One request made {used} lookups");
        seen += receipt.uncertain;
        batches += 1;
        assert!(batches <= 8, "The walk must end");
        after = receipt.next_after;
    }
    assert_eq!(seen, 12);
    assert!(batches >= 3);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn unknown_library_lengths_never_corroborate() {
    let library =
        NameLibrary::prepare(&[(CREDIT, "Morning Light", 0), (CREDIT, "Second Wind", 500)]).await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[("Morning Light", Some(0)), ("Second Wind", Some(500))]),
    );
    let receipt = library.run().await;
    assert_eq!((receipt.matched, receipt.uncertain), (0, 2));
    library.origin.shutdown().await;
}

fn partial_names(count: usize, score: u32) -> Vec<(String, String, u32)> {
    (0..count)
        .map(|index| {
            (
                format!("0f700000-0000-4000-8000-{:012x}", 100 + index),
                format!("Nova Quartet & Guest {index}"),
                score,
            )
        })
        .collect()
}

#[tokio::test]
async fn a_search_whose_page_ends_below_the_exact_score_is_conclusive_however_long_the_result_list()
{
    let library = NameLibrary::nova().await;
    let mut found = vec![(NOVA.to_string(), CREDIT.to_string(), 100)];
    found.extend(partial_names(99, 80));
    library.musicbrainz.search_scored(CREDIT, found, 4_000);
    library.musicbrainz.recordings_of(NOVA, corroborating());
    let receipt = library.run().await;
    assert_eq!((receipt.matched, receipt.uncertain), (3, 1));
    assert_eq!(
        library.associations(&library.recordings[0]).await[0].provider_artist_id,
        NOVA
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_search_page_full_of_maximum_scores_may_hide_an_exact_name_and_stays_uncertain() {
    let library = NameLibrary::nova().await;
    let mut found = vec![(NOVA.to_string(), CREDIT.to_string(), 100)];
    found.extend(partial_names(99, 100));
    library.musicbrainz.search_scored(CREDIT, found, 150);
    library.musicbrainz.recordings_of(NOVA, corroborating());
    let receipt = library.run().await;
    assert_eq!((receipt.matched, receipt.uncertain), (0, 4));
    library.origin.shutdown().await;
}

const MAIN: &str = "0f700000-0000-4000-8000-000000000a01";
const SECOND: &str = "0f700000-0000-4000-8000-000000000a02";
const THIRD: &str = "0f700000-0000-4000-8000-000000000a03";

/// Tracks "Song 0" to "Song {count-1}" of one credit, each with its own length.
fn songs(count: u64) -> Vec<(String, u64)> {
    (0..count)
        .map(|index| (format!("Song {index}"), 100_000 + index * 1000))
        .collect()
}

async fn credit_of(tracks: &[(String, u64)]) -> NameLibrary {
    let entries: Vec<(&str, &str, u64)> = tracks
        .iter()
        .map(|(title, length)| (CREDIT, title.as_str(), *length))
        .collect();
    NameLibrary::prepare(&entries).await
}

/// An artist with thousands of recordings: the library's songs among remixes, live versions and
/// unrelated recordings.
fn prolific(listed: &[(String, u64)], filler: usize) -> Reply {
    let mut items: Vec<(String, Option<u64>)> = (0..filler)
        .map(|index| (format!("Other Song {index}"), Some(100_000 + index as u64)))
        .collect();
    for (title, length) in listed {
        items.push((title.clone(), Some(*length)));
        items.push((format!("{title} (live)"), Some(*length)));
    }
    Reply::Discography(items)
}

fn three_exact_names() -> Reply {
    Reply::Scored(
        vec![
            (MAIN.into(), CREDIT.into(), 100),
            (SECOND.into(), CREDIT.into(), 73),
            (THIRD.into(), CREDIT.into(), 72),
            (TRIBUTE.into(), "Nova Quartet Tribute".into(), 49),
        ],
        4,
    )
}

#[tokio::test]
async fn three_exact_namesakes_scoring_100_73_and_72_match_the_prolific_one_and_its_credit() {
    let tracks = songs(50);
    let library = credit_of(&tracks).await;
    library.musicbrainz.search(CREDIT, three_exact_names());
    library
        .musicbrainz
        .recordings_of(MAIN, prolific(&tracks[..45], 4_000));
    library
        .musicbrainz
        .recordings_of(SECOND, nova_discography(&[("Unrelated", Some(50_000))]));
    library
        .musicbrainz
        .recordings_of(THIRD, nova_discography(&[("Song 3", Some(5_000))]));

    let mut after = Some("name:".to_string());
    let mut batches = 0;
    let (mut matched, mut uncertain, mut unavailable) = (0, 0, 0);
    while let Some(cursor) = after {
        let before = library.musicbrainz.requests().len();
        let receipt = library.request(Some(cursor)).await.unwrap();
        let used = library.musicbrainz.requests().len() - before;
        assert!(used <= 8, "One request made {used} lookups");
        (matched, uncertain, unavailable) = (
            matched + receipt.matched,
            uncertain + receipt.uncertain,
            unavailable + receipt.unavailable,
        );
        batches += 1;
        assert!(batches <= 10, "The walk must end");
        after = receipt.next_after;
    }
    assert!(
        batches >= 2,
        "A credit of 50 tracks does not fit one request"
    );
    assert_eq!(
        (matched, unavailable),
        (50, 0),
        "Every listed track follows the credit, and the artist covers 45 of the 50 probed tracks, so the five it does not list follow by coverage"
    );
    assert!(
        uncertain <= 5,
        "Only tracks the artist does not list were ever uncertain, before the walk covered them"
    );
    for (index, recording) in library.recordings.iter().enumerate() {
        let linked = library.associations(recording).await;
        assert_eq!(linked[0].provider_artist_id, MAIN, "Track {index}");
        assert_eq!(
            linked[0].evidence,
            if index < 45 {
                ArtistAssociationEvidence::MatchedNameDiscography
            } else {
                ArtistAssociationEvidence::MatchedCreditCoverage
            }
        );
    }
    let groups: std::collections::HashSet<String> = {
        let mut found = std::collections::HashSet::new();
        for recording in &library.recordings {
            found.insert(library.group_of(recording).await.unwrap());
        }
        found
    };
    assert_eq!(groups.len(), 1, "All tracks share one reviewed artist");
    assert_eq!(
        probes_of(&library, SECOND).len(),
        probes_of(&library, THIRD).len()
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn two_exact_namesakes_that_both_corroborate_stay_uncertain_whatever_their_scores() {
    let tracks = songs(8);
    let library = credit_of(&tracks).await;
    library.musicbrainz.search(CREDIT, three_exact_names());
    library
        .musicbrainz
        .recordings_of(MAIN, prolific(&tracks, 100));
    library
        .musicbrainz
        .recordings_of(SECOND, prolific(&tracks, 100));
    library
        .musicbrainz
        .recordings_of(THIRD, nova_discography(&[("Unrelated", Some(50_000))]));
    let receipt = library.run().await;
    assert_eq!((receipt.matched, receipt.uncertain), (0, 8));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_lower_scored_exact_namesake_that_corroborates_alone_is_the_artist() {
    let tracks = songs(6);
    let library = credit_of(&tracks).await;
    library.musicbrainz.search(CREDIT, three_exact_names());
    library
        .musicbrainz
        .recordings_of(MAIN, nova_discography(&[("Unrelated", Some(50_000))]));
    library
        .musicbrainz
        .recordings_of(SECOND, prolific(&tracks, 10));
    library
        .musicbrainz
        .recordings_of(THIRD, nova_discography(&[]));
    assert_eq!(library.run().await.matched, 6);
    assert_eq!(
        library.associations(&library.recordings[0]).await[0].provider_artist_id,
        SECOND
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn only_the_library_titles_picked_by_the_rule_are_sent_to_each_namesake() {
    let tracks = songs(40);
    let library = credit_of(&tracks).await;
    library.musicbrainz.search(CREDIT, three_exact_names());
    for artist in [MAIN, SECOND, THIRD] {
        library
            .musicbrainz
            .recordings_of(artist, prolific(&tracks, 10));
    }
    library.request(Some("name:".into())).await.unwrap();
    let first = probes_of(&library, MAIN);
    assert_eq!(
        first.len(),
        1,
        "The first batch asks each namesake once before following"
    );
    let decoded = reqwest_decode(&first[0]);
    assert_eq!(decoded.matches("recording:").count(), 6, "{decoded}");
    assert!(decoded.starts_with(&format!("arid:{MAIN} AND (")));
    assert!(decoded.matches("dur:[").count() == 6);
    library.origin.shutdown().await;
}

fn reqwest_decode(target: &str) -> String {
    let query = target.split_once("query=").unwrap().1;
    let query = query.split('&').next().unwrap();
    let mut decoded = String::new();
    let bytes = query.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => decoded.push(' '),
            b'%' => {
                decoded.push(u8::from_str_radix(&query[index + 1..index + 3], 16).unwrap() as char);
                index += 2;
            }
            other => decoded.push(other as char),
        }
        index += 1;
    }
    decoded
}

#[tokio::test]
async fn an_outage_while_following_keeps_the_matches_and_retries_the_rest_later() {
    let tracks = songs(20);
    let library = credit_of(&tracks).await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(MAIN, CREDIT)]));
    library
        .musicbrainz
        .recordings_of(MAIN, prolific(&tracks, 50));
    library.musicbrainz.recordings_fail_after(MAIN, 1);
    let first = library.run().await;
    assert_eq!(
        first.matched, 6,
        "The sample was answered before the outage"
    );
    assert_eq!(
        first.unavailable, 14,
        "The tracks not asked about are retried later"
    );
    assert_eq!(first.uncertain, 0);
    assert_eq!(library.rows("artist_name_checks").await, 0);

    library.musicbrainz.recordings_fail_after(MAIN, 100);
    let second = library.run().await;
    assert_eq!((second.matched, second.unavailable), (14, 0));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn name_checks_of_an_earlier_rule_are_asked_again() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(TRIBUTE, "Nova Quartet Tribute")]));
    let first = library.run().await;
    assert_eq!((first.matched, first.uncertain), (0, 4));
    assert_eq!(library.rows("artist_name_checks").await, 4);
    assert_eq!(
        library
            .scalar("SELECT count(*) FROM artist_name_checks WHERE rule=4")
            .await,
        "4",
        "Current checks carry the current rule"
    );
    assert_eq!(
        library.run().await.uncertain,
        0,
        "A current check is not asked again"
    );

    library
        .execute("UPDATE artist_name_checks SET rule=1")
        .await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(NOVA, corroborating());
    let again = library.run().await;
    assert_eq!(
        (again.matched, again.uncertain),
        (3, 1),
        "Checks decided under an earlier rule are examined again"
    );
    assert_eq!(
        library
            .scalar("SELECT count(*) FROM artist_name_checks WHERE rule=4")
            .await,
        "1"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_namesake_credit_gives_the_other_artists_tracks_nothing() {
    let tracks: Vec<(String, u64)> = (0..6)
        .map(|index| {
            let title = if index < 3 {
                format!("Alpha {index}")
            } else {
                format!("Beta {index}")
            };
            (title, 100_000 + index * 1000)
        })
        .collect();
    let library = credit_of(&tracks).await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT), (NAMESAKE, CREDIT)]));
    library.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[
            ("Alpha 0", Some(100_000)),
            ("Alpha 1", Some(101_000)),
            ("Alpha 2", Some(102_000)),
        ]),
    );
    library
        .musicbrainz
        .recordings_of(NAMESAKE, nova_discography(&[("Unrelated", Some(50_000))]));
    let receipt = library.run().await;
    assert_eq!(receipt.matched, 3);
    for (index, recording) in library.recordings.iter().enumerate() {
        assert_eq!(
            library.group_of(recording).await.is_some(),
            index < 3,
            "Track {index}: only the other artist's own recordings are matched"
        );
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_prolific_artist_is_asked_about_titles_and_never_paged() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    let listed: Vec<(String, u64)> = vec![
        ("Morning Light".into(), 201_500),
        ("Second Wind".into(), 187_000),
    ];
    library
        .musicbrainz
        .recordings_of(NOVA, prolific(&listed, 5_000));
    let receipt = library.run().await;
    assert_eq!((receipt.matched, receipt.uncertain), (2, 2));
    assert_eq!(probes_of(&library, NOVA).len(), 1);
    assert!(
        !library
            .musicbrainz
            .requests()
            .iter()
            .any(|request| request.contains("offset=100")),
        "No page of the artist's recordings is read"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn why_a_lookup_was_unavailable_is_logged_as_a_fixed_word_without_names_or_titles() {
    use hito_core::test_support::metadata_tag_logs::CapturedLogs;
    use tracing::instrument::WithSubscriber;

    let library = NameLibrary::nova().await;
    library.musicbrainz.search(CREDIT, Reply::Unavailable);
    let logs = CapturedLogs::default();
    let receipt = library
        .request(Some("name:".into()))
        .with_subscriber(tracing::Dispatch::new(logs.subscriber()))
        .await
        .unwrap();
    assert_eq!(receipt.unavailable, 4);
    let text = logs.text();
    assert!(text.contains("\"reason\":\"status_503\""), "{text}");
    for private in [CREDIT, "Morning Light", NOVA] {
        assert!(!text.contains(private), "logs must not carry {private}");
    }
    library.origin.shutdown().await;
}

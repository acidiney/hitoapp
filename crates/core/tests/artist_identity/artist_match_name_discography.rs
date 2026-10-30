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

const SEARCH: &str = "/ws/2/artist?query=%22Nova+Quartet%22&fmt=json&limit=10&offset=0";

fn recordings_request(artist: &str) -> String {
    format!("/ws/2/recording?artist={artist}&fmt=json&limit=100&offset=0")
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
        "A track MusicBrainz does not corroborate stays separate"
    );
    assert_eq!(
        library.musicbrainz.requests(),
        vec![SEARCH.to_string(), recordings_request(NOVA)],
        "A name that differs from the credit is never browsed"
    );
    assert_eq!(library.rows("artist_association_name_matches").await, 3);
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
    library.musicbrainz.recordings_of(
        NAMESAKE,
        Reply::Discography(250, vec![("Unrelated".into(), Some(1000))]),
    );

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
        "Every corroborated track but the decided one"
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

#[tokio::test]
async fn a_search_with_more_results_than_the_page_is_never_conclusive() {
    let library = NameLibrary::nova().await;
    let mut found: Vec<(String, String)> = vec![(NOVA.into(), CREDIT.into())];
    for index in 0..9 {
        found.push((
            format!("0f700000-0000-4000-8000-{:012x}", 100 + index),
            format!("Nova Quartet Tribute {index}"),
        ));
    }
    library.musicbrainz.search_total(CREDIT, found, 11);
    library.musicbrainz.recordings_of(NOVA, corroborating());
    let receipt = library.run().await;
    assert_eq!((receipt.matched, receipt.uncertain), (0, 4));
    library.origin.shutdown().await;
}

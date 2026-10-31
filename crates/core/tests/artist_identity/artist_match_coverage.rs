#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use library::{CREDIT, Evidence, NAMESAKE, NOVA, NameLibrary, TRIBUTE, nova_discography};

fn title(index: usize) -> String {
    format!("Track {index:03}")
}

fn length(index: usize) -> u64 {
    200_000 + index as u64 * 7_000
}

fn entries(name: &str, count: usize) -> Vec<(String, String, u64, Evidence)> {
    (0..count)
        .map(|index| {
            (
                name.to_owned(),
                title(index),
                length(index),
                Evidence::default(),
            )
        })
        .collect()
}

async fn credit(tracks: Vec<(String, String, u64, Evidence)>) -> NameLibrary {
    let entries: Vec<(&str, &str, u64, Evidence)> = tracks
        .iter()
        .map(|(credit, title, length, evidence)| {
            (credit.as_str(), title.as_str(), *length, evidence.clone())
        })
        .collect();
    NameLibrary::prepare_with_evidence(&entries).await
}

/// The artist lists the tracks with these indexes, each with the library's length.
fn lists(indexes: impl Iterator<Item = usize>) -> Reply {
    let titles: Vec<(String, Option<u64>)> = indexes
        .map(|index| (title(index), Some(length(index))))
        .collect();
    let borrowed: Vec<(&str, Option<u64>)> = titles
        .iter()
        .map(|(title, length)| (title.as_str(), *length))
        .collect();
    nova_discography(&borrowed)
}

fn only_nova(library: &NameLibrary, name: &str) {
    library
        .musicbrainz
        .search(name, Reply::Candidates(vec![(NOVA.into(), name.into())]));
}

async fn evidence(library: &NameLibrary, index: usize) -> String {
    let linked = library.associations(&library.recordings[index]).await;
    format!("{:?}", linked[0].evidence)
}

async fn undecided(library: &NameLibrary) -> usize {
    let mut count = 0;
    for recording in &library.recordings {
        if library.group_of(recording).await.is_none() {
            count += 1;
        }
    }
    count
}

#[tokio::test]
async fn a_credit_whose_artist_lists_most_of_its_tracks_takes_the_rest_with_its_own_provenance() {
    let library = credit(entries(CREDIT, 6)).await;
    only_nova(&library, CREDIT);
    library.musicbrainz.recordings_of(NOVA, lists(0..5));
    library.run().await;
    assert_eq!(undecided(&library).await, 0, "The unlisted track follows");
    for index in 0..5 {
        assert_eq!(evidence(&library, index).await, "MatchedNameDiscography");
    }
    assert_eq!(evidence(&library, 5).await, "MatchedCreditCoverage");
    let followed = &library.associations(&library.recordings[5]).await[0];
    assert_eq!(followed.provider_artist_id, NOVA);
    let coverage = followed.coverage_evidence.unwrap();
    assert_eq!((coverage.matched_tracks, coverage.probed_tracks), (5, 6));
    assert_eq!(library.rows("artist_association_coverage_matches").await, 1);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_prolific_credit_walked_in_many_requests_covers_its_remaining_tracks() {
    let library = credit(entries(CREDIT, 100)).await;
    only_nova(&library, CREDIT);
    library.musicbrainz.recordings_of(NOVA, lists(0..80));
    library.run().await;
    assert_eq!(undecided(&library).await, 0);
    let covered = library.rows("artist_association_coverage_matches").await;
    assert_eq!(
        covered, 20,
        "Exactly the tracks the artist does not list follow by coverage"
    );
    for index in 80..100 {
        assert_eq!(evidence(&library, index).await, "MatchedCreditCoverage");
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn too_few_titles_or_too_small_a_share_of_the_probed_tracks_follows_nothing() {
    for (count, listed) in [(6, 3), (5, 4)] {
        let library = credit(entries(CREDIT, count)).await;
        only_nova(&library, CREDIT);
        library.musicbrainz.recordings_of(NOVA, lists(0..listed));
        library.run().await;
        assert_eq!(
            undecided(&library).await,
            count - listed,
            "{count} tracks, {listed} listed: only the listed tracks match"
        );
        assert_eq!(library.rows("artist_association_coverage_matches").await, 0);
        library.origin.shutdown().await;
    }
}

#[tokio::test]
async fn a_namesake_that_corroborates_any_title_keeps_the_rest_out() {
    let library = credit(entries(CREDIT, 6)).await;
    library.musicbrainz.search(
        CREDIT,
        Reply::Candidates(vec![
            (NOVA.into(), CREDIT.into()),
            (NAMESAKE.into(), CREDIT.into()),
        ]),
    );
    library.musicbrainz.recordings_of(NOVA, lists(0..5));
    library.musicbrainz.recordings_of(NAMESAKE, lists(0..1));
    library.run().await;
    assert_eq!(undecided(&library).await, 1);
    assert!(library.group_of(&library.recordings[5]).await.is_none());
    assert_eq!(library.rows("artist_association_coverage_matches").await, 0);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_credit_naming_several_artists_follows_nothing() {
    let name = "Nova Quartet and Guest";
    let library = credit(entries(name, 6)).await;
    only_nova(&library, name);
    library.musicbrainz.recordings_of(NOVA, lists(0..5));
    library.run().await;
    assert_eq!(undecided(&library).await, 1);
    assert_eq!(library.rows("artist_association_coverage_matches").await, 0);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn strong_evidence_naming_another_artist_in_the_credit_keeps_the_rest_out() {
    let mut tracks = entries(CREDIT, 8);
    for (index, (.., evidence)) in tracks.iter_mut().enumerate().take(2) {
        evidence.isrc = Some(format!("GBAAA20{index:05}"));
    }
    let library = credit(tracks).await;
    for (index, artist) in [(0, NAMESAKE), (1, TRIBUTE)] {
        library.musicbrainz.isrc(
            &format!("GBAAA20{index:05}"),
            Reply::Recordings(vec![(
                "0f700000-0000-4000-8000-0000000000aa".into(),
                vec![artist.to_owned()],
            )]),
        );
    }
    only_nova(&library, CREDIT);
    library.musicbrainz.recordings_of(NOVA, lists(2..7));
    library.run().await;
    assert_eq!(library.rows("artist_association_credit_matches").await, 0);
    assert_eq!(library.rows("artist_association_coverage_matches").await, 0);
    assert!(
        library.group_of(&library.recordings[7]).await.is_none(),
        "Two tracks name two other artists, so the credit's remainder is not handed to a third"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn name_checks_of_the_earlier_rule_do_not_hide_a_track() {
    let library = credit(entries(CREDIT, 6)).await;
    only_nova(&library, CREDIT);
    library.run().await;
    assert_eq!(undecided(&library).await, 6);
    library
        .execute("UPDATE artist_name_checks SET rule=2")
        .await;
    library.musicbrainz.recordings_of(NOVA, lists(0..6));
    library.run().await;
    assert_eq!(undecided(&library).await, 0);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_namesake_listing_a_title_matched_in_an_earlier_request_keeps_the_rest_out() {
    for (namesake_lists_earlier_title, followed) in [(true, 0), (false, 1)] {
        let library = credit(entries(CREDIT, 6)).await;
        library.musicbrainz.search(
            CREDIT,
            Reply::Candidates(vec![
                (NOVA.into(), CREDIT.into()),
                (NAMESAKE.into(), CREDIT.into()),
            ]),
        );
        library.musicbrainz.recordings_of(NOVA, lists(0..3));
        library.musicbrainz.recordings_of(
            NAMESAKE,
            if namesake_lists_earlier_title {
                lists(0..1)
            } else {
                lists(0..0)
            },
        );
        library.run().await;
        assert_eq!(undecided(&library).await, 3, "Three of six is not enough");

        library
            .execute("UPDATE artist_name_checks SET rule=2")
            .await;
        library.musicbrainz.recordings_of(NOVA, lists(0..5));
        library.run().await;
        assert_eq!(
            undecided(&library).await,
            1 - followed,
            "The artist lists two more titles; the sixth track follows only beside silent namesakes"
        );
        assert_eq!(
            library.rows("artist_association_coverage_matches").await,
            followed as i64
        );
        library.origin.shutdown().await;
    }
}

#[tokio::test]
async fn a_namesake_listing_a_title_of_the_tail_keeps_the_tail_out() {
    for _ in 0..3 {
        let library = credit(entries(CREDIT, 20)).await;
        library.musicbrainz.search(
            CREDIT,
            Reply::Candidates(vec![
                (NOVA.into(), CREDIT.into()),
                (NAMESAKE.into(), CREDIT.into()),
            ]),
        );
        library.musicbrainz.recordings_of(NOVA, lists(0..15));
        library.musicbrainz.recordings_of(NAMESAKE, lists(17..18));
        library.run().await;
        for index in 15..20 {
            assert!(
                library.group_of(&library.recordings[index]).await.is_none(),
                "Track {index} belongs to nobody the walk can name"
            );
        }
        assert_eq!(library.rows("artist_association_coverage_matches").await, 0);
        library.origin.shutdown().await;
    }
}

/// A track decided by the account for another artist, by removing the provenance of a name match.
async fn choose_other_artist(library: &NameLibrary) {
    library
        .execute("DROP TRIGGER artist_association_name_match_retained")
        .await;
    library
        .execute("DELETE FROM artist_association_name_matches")
        .await;
    library
        .execute("UPDATE artist_name_checks SET rule=2")
        .await;
}

#[tokio::test]
async fn a_personal_choice_for_another_artist_in_the_credit_keeps_the_tail_out() {
    // The first request samples six of the eight tracks, so the other artist needs both of its
    // titles in the sample; libraries are built again until it has them.
    let library = loop {
        let library = credit(entries(CREDIT, 8)).await;
        library.musicbrainz.search(
            CREDIT,
            Reply::Candidates(vec![(NAMESAKE.into(), CREDIT.into())]),
        );
        library.musicbrainz.recordings_of(NAMESAKE, lists(6..8));
        library.run().await;
        if library.group_of(&library.recordings[6]).await.is_some() {
            break library;
        }
        library.origin.shutdown().await;
    };
    choose_other_artist(&library).await;

    only_nova(&library, CREDIT);
    library.musicbrainz.recordings_of(NOVA, lists(0..5));
    library.run().await;
    for index in 0..5 {
        assert!(library.group_of(&library.recordings[index]).await.is_some());
    }
    assert!(
        library.group_of(&library.recordings[5]).await.is_none(),
        "The account's own choice for another artist stops the credit's tail following"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn strong_evidence_for_other_artists_blocks_name_matches_of_the_credit() {
    let mut tracks = entries(CREDIT, 8);
    for (index, (.., evidence)) in tracks.iter_mut().enumerate().take(2) {
        evidence.isrc = Some(format!("GBAAA20{index:05}"));
    }
    let library = credit(tracks).await;
    for (index, artist) in [(0, NAMESAKE), (1, TRIBUTE)] {
        library.musicbrainz.isrc(
            &format!("GBAAA20{index:05}"),
            Reply::Recordings(vec![(
                "0f700000-0000-4000-8000-0000000000aa".into(),
                vec![artist.to_owned()],
            )]),
        );
    }
    only_nova(&library, CREDIT);
    library.musicbrainz.recordings_of(NOVA, lists(2..8));
    library.run().await;
    assert_eq!(library.rows("artist_association_name_matches").await, 0);
    for index in 2..8 {
        assert!(library.group_of(&library.recordings[index]).await.is_none());
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn name_matches_of_an_earlier_rule_do_not_count_toward_coverage() {
    let library = credit(entries(CREDIT, 7)).await;
    only_nova(&library, CREDIT);
    library.musicbrainz.recordings_of(NOVA, lists(0..3));
    library.run().await;
    assert_eq!(undecided(&library).await, 4);
    library
        .execute("DROP TRIGGER artist_association_name_match_immutable")
        .await;
    library
        .execute("UPDATE artist_association_name_matches SET rule=2")
        .await;
    library
        .execute("UPDATE artist_name_checks SET rule=2")
        .await;
    library.musicbrainz.recordings_of(NOVA, lists(0..6));
    library.run().await;
    assert_eq!(
        undecided(&library).await,
        1,
        "Three current matches are too few titles to cover the seventh track"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_title_too_long_to_ask_about_is_left_out_and_does_not_make_the_credit_unavailable() {
    let mut tracks = entries(CREDIT, 4);
    tracks[3].1 = "x".repeat(600);
    let library = credit(tracks).await;
    only_nova(&library, CREDIT);
    library.musicbrainz.recordings_of(NOVA, lists(0..3));
    let receipt = library.run().await;
    assert_eq!(receipt.unavailable, 0);
    assert_eq!(receipt.matched, 3);
    assert_eq!(library.rows("artist_name_checks").await, 1);
    library.origin.shutdown().await;
}

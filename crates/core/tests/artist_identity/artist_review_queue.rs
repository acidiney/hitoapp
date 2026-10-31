#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{
    ArtistAssociationEvidence, ArtistReviewItem, ArtistReviewQueueQuery,
    ConfirmArtistReviewCandidate, DismissArtistReviewCandidates, Error,
    ResetRecordingArtistIdentity,
};
use library::{CREDIT, NAMESAKE, NOVA, NameLibrary, TRIBUTE, nova_discography};
use uuid::Uuid;

fn candidates(entries: &[(&str, &str)]) -> Reply {
    Reply::Candidates(
        entries
            .iter()
            .map(|(id, name)| ((*id).into(), (*name).into()))
            .collect(),
    )
}

async fn queue(library: &NameLibrary) -> Vec<ArtistReviewItem> {
    library
        .hito
        .artist_catalog
        .artist_review_queue(&library.actor, ArtistReviewQueueQuery::default())
        .await
        .unwrap()
        .items
}

/// Two namesakes, neither of which lists enough of the library to corroborate it.
async fn undecided_namesakes() -> NameLibrary {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT), (NAMESAKE, CREDIT)]));
    library.musicbrainz.describe(NOVA, "Lisbon fado band", "PT");
    library
        .musicbrainz
        .describe(NAMESAKE, "Brazilian jazz trio", "BR");
    library
        .musicbrainz
        .recordings_of(NOVA, nova_discography(&[("Morning Light", Some(201_000))]));
    library
        .musicbrainz
        .recordings_of(NAMESAKE, nova_discography(&[]));
    library.run().await;
    library
}

async fn credit_id(library: &NameLibrary) -> String {
    let items = queue(library).await;
    assert_eq!(items.len(), 1, "The walk queued the credit");
    items[0].artist_id.clone()
}

async fn confirm(
    library: &NameLibrary,
    credit: &str,
    artist: &str,
) -> hito_core::Result<hito_core::ArtistReviewConfirmation> {
    library
        .hito
        .artist_catalog
        .confirm_artist_review_candidate(
            &library.actor,
            credit,
            ConfirmArtistReviewCandidate {
                request_id: Uuid::new_v4(),
                provider_artist_id: artist.into(),
            },
        )
        .await
}

#[tokio::test]
async fn namesakes_the_walk_could_not_tell_apart_are_queued_with_what_it_stored() {
    let library = undecided_namesakes().await;
    let items = queue(&library).await;
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!((item.name.as_str(), item.undecided_tracks), (CREDIT, 4));
    let found: Vec<(&str, i64, Option<&str>, Option<&str>, Option<&str>)> = item
        .candidates
        .iter()
        .map(|candidate| {
            (
                candidate.provider_artist_id.as_str(),
                candidate.corroborated_titles,
                candidate.disambiguation.as_deref(),
                candidate.country.as_deref(),
                candidate.kind.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        found,
        vec![
            (
                NOVA,
                1,
                Some("Lisbon fado band"),
                Some("PT"),
                Some("person")
            ),
            (
                NAMESAKE,
                0,
                Some("Brazilian jazz trio"),
                Some("BR"),
                Some("person")
            ),
        ],
        "The better corroborated namesake comes first"
    );
    let asked = library.musicbrainz.requests().len();
    queue(&library).await;
    assert_eq!(
        library.musicbrainz.requests().len(),
        asked,
        "Reading the queue asks MusicBrainz nothing"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_matched_credit_and_a_credit_without_namesakes_are_not_queued() {
    let matched = NameLibrary::nova().await;
    matched
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    matched.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
        ]),
    );
    matched.run().await;
    assert!(queue(&matched).await.is_empty(), "A matched credit is done");

    let unknown = NameLibrary::nova().await;
    unknown
        .musicbrainz
        .search(CREDIT, candidates(&[(TRIBUTE, "Nova Quartet Tribute")]));
    unknown.run().await;
    assert!(
        queue(&unknown).await.is_empty(),
        "With no exact-name artist there is nobody to confirm"
    );
    assert!(
        queue(&NameLibrary::nova().await).await.is_empty(),
        "Before the walk"
    );
    matched.origin.shutdown().await;
    unknown.origin.shutdown().await;
}

#[tokio::test]
async fn the_queue_is_paged_and_counts_every_credit() {
    let entries: Vec<(&str, &str, u64)> = vec![
        ("Alpha Band", "One", 200_000),
        ("Alpha Band", "Two", 210_000),
        ("Beta Band", "Three", 220_000),
        ("Beta Band", "Four", 230_000),
        ("Beta Band", "Five", 240_000),
    ];
    let library = NameLibrary::prepare(&entries).await;
    for name in ["Alpha Band", "Beta Band"] {
        library
            .musicbrainz
            .search(name, candidates(&[(NOVA, name), (NAMESAKE, name)]));
    }
    library
        .musicbrainz
        .recordings_of(NOVA, nova_discography(&[]));
    library
        .musicbrainz
        .recordings_of(NAMESAKE, nova_discography(&[]));
    library.run().await;
    let first = library
        .hito
        .artist_catalog
        .artist_review_queue(
            &library.actor,
            ArtistReviewQueueQuery {
                offset: Some(0),
                limit: Some(1),
            },
        )
        .await
        .unwrap();
    assert_eq!((first.total, first.items.len()), (2, 1));
    assert_eq!(
        first.items[0].name, "Beta Band",
        "The larger credit comes first"
    );
    let second = library
        .hito
        .artist_catalog
        .artist_review_queue(
            &library.actor,
            ArtistReviewQueueQuery {
                offset: Some(1),
                limit: Some(1),
            },
        )
        .await
        .unwrap();
    assert_eq!(second.items[0].name, "Alpha Band");
    library.origin.shutdown().await;
}

#[tokio::test]
async fn one_click_applies_the_candidate_to_the_undecided_tracks_with_its_own_provenance() {
    let library = undecided_namesakes().await;
    let credit = credit_id(&library).await;
    let receipt = confirm(&library, &credit, NOVA).await.unwrap();
    assert_eq!(
        (receipt.applied, receipt.kept, receipt.remaining),
        (4, 0, 0)
    );
    for recording in &library.recordings {
        let associations = library.associations(recording).await;
        assert_eq!(associations[0].provider_artist_id, NOVA);
        assert_eq!(
            associations[0].evidence,
            ArtistAssociationEvidence::UserConfirmedCandidate
        );
        assert!(library.group_of(recording).await.is_some());
    }
    assert!(queue(&library).await.is_empty(), "The row leaves the queue");
    let chosen = library
        .hito
        .artist_catalog
        .artist_identity_groups(&library.actor, Default::default())
        .await
        .unwrap();
    assert_eq!(
        chosen.items.len(),
        1,
        "The artist appears among the chosen artists"
    );
    assert_eq!(chosen.items[0].provider_artist_id, NOVA);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_confirmation_replays_and_a_personal_decision_wins() {
    let library = undecided_namesakes().await;
    let credit = credit_id(&library).await;
    let decided = &library.recordings[0];
    let choice = library
        .hito
        .artist_catalog
        .recording_artist_identity_choice(&library.actor, decided)
        .await
        .unwrap();
    library
        .hito
        .artist_catalog
        .reset_recording_artist_identity(
            &library.actor,
            decided,
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
            },
        )
        .await
        .unwrap();
    let request = ConfirmArtistReviewCandidate {
        request_id: Uuid::new_v4(),
        provider_artist_id: NOVA.into(),
    };
    let first = library
        .hito
        .artist_catalog
        .confirm_artist_review_candidate(&library.actor, &credit, request.clone())
        .await
        .unwrap();
    assert_eq!(
        (first.applied, first.remaining),
        (3, 0),
        "The track the account reset is left alone"
    );
    assert!(library.group_of(decided).await.is_none());
    let replay = library
        .hito
        .artist_catalog
        .confirm_artist_review_candidate(&library.actor, &credit, request.clone())
        .await
        .unwrap();
    assert_eq!(replay, first);
    let other = ConfirmArtistReviewCandidate {
        provider_artist_id: NAMESAKE.into(),
        ..request
    };
    let conflict = library
        .hito
        .artist_catalog
        .confirm_artist_review_candidate(&library.actor, &credit, other)
        .await;
    assert!(matches!(conflict, Err(Error::Conflict(_))));
    assert!(
        queue(&library).await.is_empty(),
        "A credit whose remaining tracks the account decided itself is done"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn only_a_listed_candidate_of_the_accounts_own_credit_can_be_confirmed() {
    let library = undecided_namesakes().await;
    let credit = credit_id(&library).await;
    for (credit, artist) in [
        (credit.as_str(), TRIBUTE),
        ("missing-credit", NOVA),
        (credit.as_str(), "not-an-id"),
    ] {
        assert!(matches!(
            confirm(&library, credit, artist).await,
            Err(Error::NotFound)
        ));
    }
    assert!(matches!(
        confirm(&library, &credit, "").await,
        Err(Error::NotFound | Error::Invalid(_))
    ));
    assert_eq!(queue(&library).await.len(), 1, "Nothing changed");
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_dismissed_candidate_leaves_the_queue_and_the_walk_does_not_bring_it_back() {
    let library = undecided_namesakes().await;
    let credit = credit_id(&library).await;
    let dismissal = library
        .hito
        .artist_catalog
        .dismiss_artist_review_candidates(
            &library.actor,
            &credit,
            DismissArtistReviewCandidates {
                provider_artist_id: Some(NOVA.into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(dismissal.dismissed, 1);
    let items = queue(&library).await;
    let left: Vec<&str> = items[0]
        .candidates
        .iter()
        .map(|candidate| candidate.provider_artist_id.as_str())
        .collect();
    assert_eq!(left, vec![NAMESAKE]);
    let none = library
        .hito
        .artist_catalog
        .dismiss_artist_review_candidates(
            &library.actor,
            &credit,
            DismissArtistReviewCandidates {
                provider_artist_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        none.dismissed, 1,
        "None of these dismisses what is still listed"
    );
    assert!(queue(&library).await.is_empty());
    library.run().await;
    assert!(
        queue(&library).await.is_empty(),
        "A later walk keeps the dismissals"
    );
    assert!(matches!(
        confirm(&library, &credit, NOVA).await,
        Err(Error::NotFound)
    ));
    library.origin.shutdown().await;
}

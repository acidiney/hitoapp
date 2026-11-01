#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{
    Actor, ArtistAssociationEvidence, ArtistReviewCandidate, ArtistReviewItem,
    ArtistReviewQueueQuery, ConfirmArtistReviewCandidate, Credentials,
    DismissArtistReviewCandidates, Error, ResetRecordingArtistIdentity,
    RestoreArtistReviewCandidates,
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
                after: None,
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
    let expected = |id: &str, note: &str, country: &str, titles: i64| ArtistReviewCandidate {
        provider_artist_id: id.into(),
        name: CREDIT.into(),
        kind: Some("person".into()),
        disambiguation: Some(note.into()),
        country: Some(country.into()),
        corroborated_titles: titles,
        in_library: false,
    };
    assert_eq!(
        item.candidates,
        vec![
            expected(NOVA, "Lisbon fado band", "PT", 1),
            expected(NAMESAKE, "Brazilian jazz trio", "BR", 0),
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
    let read = library
        .hito
        .artist_catalog
        .artist_match_suggestions(&library.actor, &credit)
        .await
        .unwrap();
    assert_eq!(
        read.chosen[0]
            .evidence
            .iter()
            .map(|how| (how.evidence, how.tracks))
            .collect::<Vec<_>>(),
        vec![(ArtistAssociationEvidence::UserConfirmedCandidate, 4)],
        "The credit page can say the account confirmed the candidate"
    );
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
        after: None,
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

async fn dismissed(library: &NameLibrary) -> Vec<ArtistReviewItem> {
    library
        .hito
        .artist_catalog
        .dismissed_artist_reviews(&library.actor, ArtistReviewQueueQuery::default())
        .await
        .unwrap()
        .items
}

async fn restore(
    library: &NameLibrary,
    credit: &str,
    artist: Option<&str>,
) -> hito_core::Result<hito_core::ArtistReviewRestoration> {
    library
        .hito
        .artist_catalog
        .restore_artist_review_candidates(
            &library.actor,
            credit,
            RestoreArtistReviewCandidates {
                provider_artist_id: artist.map(Into::into),
            },
        )
        .await
}

#[tokio::test]
async fn dismissed_candidates_are_listed_and_restoring_them_brings_the_credit_back() {
    let library = undecided_namesakes().await;
    let credit = credit_id(&library).await;
    assert!(
        dismissed(&library).await.is_empty(),
        "Nothing is dismissed yet"
    );
    library
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
    let listed = dismissed(&library).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].artist_id.as_str(), listed[0].undecided_tracks),
        (credit.as_str(), 4)
    );
    let ids: Vec<&str> = listed[0]
        .candidates
        .iter()
        .map(|candidate| candidate.provider_artist_id.as_str())
        .collect();
    assert_eq!(ids, vec![NOVA], "Only what was dismissed is listed");

    let asked = library.musicbrainz.requests().len();
    let restored = restore(&library, &credit, Some(NOVA)).await.unwrap();
    assert_eq!(
        (restored.artist_id.as_str(), restored.restored),
        (credit.as_str(), 1)
    );
    assert!(dismissed(&library).await.is_empty());
    assert_eq!(queue(&library).await[0].candidates.len(), 2);
    assert_eq!(
        restore(&library, &credit, Some(NOVA))
            .await
            .unwrap()
            .restored,
        0
    );
    assert_eq!(
        library.musicbrainz.requests().len(),
        asked,
        "Restoring asks MusicBrainz nothing"
    );

    library
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
    assert!(queue(&library).await.is_empty());
    assert_eq!(dismissed(&library).await[0].candidates.len(), 2);
    assert_eq!(restore(&library, &credit, None).await.unwrap().restored, 2);
    assert_eq!(queue(&library).await[0].candidates.len(), 2);
    assert_eq!(restore(&library, &credit, None).await.unwrap().restored, 0);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn only_the_accounts_own_credit_and_a_listed_dismissal_can_be_restored() {
    let library = undecided_namesakes().await;
    let credit = credit_id(&library).await;
    assert!(matches!(
        restore(&library, "no-such-credit", None).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        restore(&library, &credit, Some("")).await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        restore(&library, &credit, Some(NOVA))
            .await
            .unwrap()
            .restored,
        0,
        "A candidate that was never dismissed has nothing to restore"
    );
    library.origin.shutdown().await;
}

async fn second_account(library: &NameLibrary) -> Actor {
    let credentials = || Credentials {
        username: "outsider".into(),
        password: "owned review outsider password".into(),
    };
    library
        .hito
        .identity
        .create_user(&library.actor, credentials())
        .await
        .unwrap();
    let session = library.hito.identity.login(credentials()).await.unwrap();
    library
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap()
}

#[tokio::test]
async fn another_account_cannot_restore_or_see_the_dismissals_of_this_one() {
    let library = undecided_namesakes().await;
    let credit = credit_id(&library).await;
    library
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
    let outsider = second_account(&library).await;
    let catalog = &library.hito.artist_catalog;
    for artist in [None, Some(NOVA.to_owned())] {
        assert!(matches!(
            catalog
                .restore_artist_review_candidates(
                    &outsider,
                    &credit,
                    RestoreArtistReviewCandidates {
                        provider_artist_id: artist
                    }
                )
                .await,
            Err(Error::NotFound)
        ));
    }
    let seen = catalog
        .dismissed_artist_reviews(&outsider, ArtistReviewQueueQuery::default())
        .await
        .unwrap();
    assert_eq!((seen.total, seen.items.len()), (0, 0));
    assert_eq!(
        dismissed(&library).await[0].candidates.len(),
        2,
        "The owner's dismissals are untouched"
    );
    library.origin.shutdown().await;
}

/// 260 undecided tracks of one credit, the 250 with the lowest identifiers hidden by a shared
/// correction so that applying the candidate to them is declined.
async fn mostly_declined() -> NameLibrary {
    let credit = |count: usize, offset: usize| -> Vec<(String, String, u64)> {
        (0..count)
            .map(|index| {
                (
                    CREDIT.to_owned(),
                    format!("Track {:03}", offset + index),
                    200_000 + (offset + index) as u64 * 1_000,
                )
            })
            .collect()
    };
    let borrowed = |entries: &[(String, String, u64)]| -> Vec<(&'static str, &'static str, u64)> {
        entries
            .iter()
            .map(|(a, b, c)| {
                (
                    &*Box::leak(a.clone().into_boxed_str()),
                    &*Box::leak(b.clone().into_boxed_str()),
                    *c,
                )
            })
            .collect()
    };
    let first = credit(100, 0);
    let mut library = NameLibrary::prepare(&borrowed(&first)).await;
    library
        .import_more("second", &borrowed(&credit(100, 100)))
        .await;
    library
        .import_more("third", &borrowed(&credit(60, 200)))
        .await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT), (NAMESAKE, CREDIT)]));
    library
        .musicbrainz
        .recordings_of(NOVA, nova_discography(&[]));
    library
        .musicbrainz
        .recordings_of(NAMESAKE, nova_discography(&[]));
    library.run().await;
    library.run().await;
    let mut ids = library.recordings.clone();
    ids.sort();
    let hidden: Vec<String> = ids[..250].iter().map(|id| format!("'{id}'")).collect();
    let admin = library.actor.user().id.clone();
    library
        .execute_batch(&format!(
            "PRAGMA foreign_keys=OFF;
             INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_change_id,change_floor) VALUES('c1','{admin}','split','change',0);
             INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES((SELECT max(seq) FROM shared_artist_corrections),1,1,'{admin}');
             INSERT INTO shared_artist_correction_subjects(correction_seq,recording_id,observation_id,provider_artist_id,raw_artist)
                SELECT (SELECT max(seq) FROM shared_artist_corrections),r.id,r.metadata_observation_id,'{NOVA}','x'
                FROM recordings r WHERE r.id IN ({});",
            hidden.join(",")
        ))
        .await;
    library
}

#[tokio::test]
async fn a_credit_whose_first_tracks_all_decline_is_still_finished_and_leaves_the_queue() {
    let library = mostly_declined().await;
    let credit = credit_id(&library).await;
    assert_eq!(queue(&library).await[0].undecided_tracks, 260);
    let mut after = None;
    let (mut applied, mut kept, mut steps) = (0, 0, 0);
    loop {
        steps += 1;
        assert!(steps <= 5, "The steps must end");
        let receipt = library
            .hito
            .artist_catalog
            .confirm_artist_review_candidate(
                &library.actor,
                &credit,
                ConfirmArtistReviewCandidate {
                    request_id: Uuid::new_v4(),
                    provider_artist_id: NOVA.into(),
                    after: after.clone(),
                },
            )
            .await
            .unwrap();
        applied += receipt.applied;
        kept += receipt.kept;
        match receipt.next_after {
            Some(next) => {
                assert_ne!(Some(&next), after.as_ref(), "Each step moves forward");
                after = Some(next);
            }
            None => break,
        }
    }
    assert_eq!((applied, kept, steps), (10, 250, 2));
    assert!(
        queue(&library).await.is_empty(),
        "Nothing applicable remains, so the credit leaves the queue"
    );
    library.origin.shutdown().await;
}

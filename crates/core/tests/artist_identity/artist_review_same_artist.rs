use super::artist_review_reach::library::{CREDIT, NOVA, NameLibrary, nova_discography};
use super::artist_review_reach::{EDITED, candidates, confirm_as, dismiss, queue, queue_of};
use hito_core::{
    Action, ArtistAssociationEvidence, ArtistReviewOrigin, ArtistReviewQueueQuery, Browse,
    Credentials, DismissArtistReviewCandidates, Error, RestoreArtistReviewCandidates,
};
use hito_core::{Actor, test_support::support};
use uuid::Uuid;

pub(super) async fn second_account(library: &NameLibrary) -> Actor {
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

pub(super) async fn edit_artist(library: &NameLibrary, recording: &str, artist: &str) {
    support::synchronize(
        &library.hito,
        &library.actor,
        vec![Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(recording).unwrap(),
            artist: artist.into(),
        }],
    )
    .await;
}

pub(super) async fn chosen_and_edited(edited_name: &str) -> NameLibrary {
    let mut library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
            ("Harbor Lights", Some(243_000)),
            ("Quiet Rooms", Some(150_000)),
        ]),
    );
    library.run().await;
    library
        .import_more("local", &[("Someone Else", "Edited Song", 190_000)])
        .await;
    let edited = library.recordings.last().unwrap().clone();
    edit_artist(&library, &edited, edited_name).await;
    library
}

pub(super) async fn edited_credit(library: &NameLibrary) -> String {
    let items = queue(library).await;
    assert_eq!(items.len(), 1, "{items:?}");
    items[0].artist_id.clone()
}

#[tokio::test]
async fn a_personally_edited_credit_is_offered_the_artist_the_account_already_chose() {
    let library = chosen_and_edited(EDITED).await;
    let items = queue(&library).await;
    assert_eq!(items.len(), 1, "{items:?}");
    let item = &items[0];
    assert_eq!(
        (item.name.as_str(), item.undecided_tracks, item.needs_search),
        (EDITED, 1, false)
    );
    assert_eq!(item.candidates.len(), 1);
    let offer = &item.candidates[0];
    assert_eq!(offer.provider_artist_id, NOVA);
    assert_eq!(offer.name, CREDIT, "The offer names the chosen artist");
    assert_eq!(offer.origin, ArtistReviewOrigin::ChosenArtist);
    assert!(offer.in_library);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn nothing_is_offered_for_another_name_or_for_a_credit_that_was_not_edited() {
    let other = chosen_and_edited("Nova Quintet").await;
    assert!(
        queue(&other).await.iter().all(|item| item
            .candidates
            .iter()
            .all(|c| c.origin != ArtistReviewOrigin::ChosenArtist)),
        "A different name is not evidence"
    );
    assert!(queue(&other).await.is_empty());
    other.origin.shutdown().await;

    let mut plain = NameLibrary::nova().await;
    plain
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    plain.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
            ("Harbor Lights", Some(243_000)),
            ("Quiet Rooms", Some(150_000)),
        ]),
    );
    plain.run().await;
    plain
        .import_more("local", &[(EDITED, "Unedited Song", 190_000)])
        .await;
    assert!(
        queue(&plain)
            .await
            .iter()
            .all(|item| item.candidates.is_empty()),
        "A credit the account did not edit is left to the walk"
    );
    plain.origin.shutdown().await;
}

#[tokio::test]
async fn another_account_sees_no_offer_and_cannot_use_it() {
    let library = chosen_and_edited(EDITED).await;
    let credit = edited_credit(&library).await;
    let outsider = second_account(&library).await;
    assert!(queue_of(&library, &outsider).await.is_empty());
    assert!(matches!(
        confirm_as(&library, &outsider, &credit, NOVA).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        library
            .hito
            .artist_catalog
            .dismiss_artist_review_candidates(
                &outsider,
                &credit,
                DismissArtistReviewCandidates {
                    provider_artist_id: Some(NOVA.into())
                }
            )
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        queue(&library).await.len(),
        1,
        "The owner's offer is untouched"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn confirming_applies_the_chosen_identity_and_the_two_cards_become_one_artist() {
    let library = chosen_and_edited(EDITED).await;
    let credit = edited_credit(&library).await;
    let before = library
        .hito
        .catalog
        .artists(&library.actor, Browse::default())
        .await
        .unwrap();
    assert!(before.items.iter().any(|artist| artist.name == EDITED));
    let receipt = confirm_as(&library, &library.actor, &credit, NOVA)
        .await
        .unwrap();
    assert_eq!(
        (receipt.applied, receipt.kept, receipt.remaining),
        (1, 0, 0)
    );
    let edited = library.recordings.last().unwrap();
    let associations = library.associations(edited).await;
    assert_eq!(associations[0].provider_artist_id, NOVA);
    assert_eq!(
        associations[0].evidence,
        ArtistAssociationEvidence::UserConfirmedCandidate
    );
    assert_eq!(
        library.group_of(edited).await,
        library.group_of(&library.recordings[0]).await,
        "Both credits now share one chosen artist"
    );
    let groups = library
        .hito
        .artist_catalog
        .artist_identity_groups(&library.actor, Default::default())
        .await
        .unwrap();
    assert_eq!(groups.items.len(), 1);
    assert_eq!(groups.items[0].recording_count, 5);
    let after = library
        .hito
        .catalog
        .artists(&library.actor, Browse::default())
        .await
        .unwrap();
    assert!(
        after.items.iter().all(|artist| artist.name != EDITED),
        "The edited card is covered by the chosen artist"
    );
    assert!(queue(&library).await.is_empty());
    assert!(matches!(
        confirm_as(&library, &library.actor, &credit, NOVA).await,
        Err(Error::NotFound)
    ));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn an_edited_credit_that_already_has_an_identity_is_not_offered_again() {
    let library = chosen_and_edited(EDITED).await;
    let credit = edited_credit(&library).await;
    confirm_as(&library, &library.actor, &credit, NOVA)
        .await
        .unwrap();
    library.run().await;
    assert!(queue(&library).await.is_empty());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn dismissing_the_offer_hides_it_for_good_and_restoring_brings_it_back() {
    let library = chosen_and_edited(EDITED).await;
    let credit = edited_credit(&library).await;
    let dismissal = dismiss(&library, &credit, Some(NOVA)).await.unwrap();
    assert_eq!(dismissal.dismissed, 1);
    assert!(queue(&library).await.is_empty());
    library.run().await;
    library.automatic().await;
    assert!(
        queue(&library).await.is_empty(),
        "A walk does not bring it back"
    );
    assert!(matches!(
        confirm_as(&library, &library.actor, &credit, NOVA).await,
        Err(Error::NotFound)
    ));
    let hidden = library
        .hito
        .artist_catalog
        .dismissed_artist_reviews(&library.actor, ArtistReviewQueueQuery::default())
        .await
        .unwrap();
    assert_eq!(hidden.items.len(), 1);
    assert_eq!(
        hidden.items[0].candidates[0].origin,
        ArtistReviewOrigin::ChosenArtist
    );
    let restored = library
        .hito
        .artist_catalog
        .restore_artist_review_candidates(
            &library.actor,
            &credit,
            RestoreArtistReviewCandidates {
                provider_artist_id: Some(NOVA.into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(restored.restored, 1);
    assert_eq!(queue(&library).await.len(), 1);
    library.origin.shutdown().await;
}

use super::artist_review_reach::library::{CREDIT, NAMESAKE, NOVA, NameLibrary, nova_discography};
use super::artist_review_reach::{candidates, confirm_as, dismiss, queue, queue_of};
use super::artist_review_same_artist::second_account;
use hito_core::{ArtistReviewOrigin, ArtistReviewQueueQuery, Error};

async fn two_of_four() -> NameLibrary {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
        ]),
    );
    library.run().await;
    library
}

async fn unmatched_credit_id(library: &NameLibrary) -> String {
    let items = queue(library).await;
    assert_eq!(items.len(), 1, "{items:?}");
    items[0].artist_id.clone()
}

#[tokio::test]
async fn the_tracks_a_matched_artist_did_not_cover_are_offered_that_artist() {
    let library = two_of_four().await;
    let items = queue(&library).await;
    assert_eq!(items.len(), 1, "{items:?}");
    let item = &items[0];
    assert_eq!(
        (item.undecided_tracks, item.needs_search),
        (2, false),
        "{item:?}"
    );
    assert!(item.search_recording_id.is_none());
    assert_eq!(item.candidates.len(), 1);
    let offer = &item.candidates[0];
    assert_eq!(offer.provider_artist_id, NOVA);
    assert_eq!(offer.origin, ArtistReviewOrigin::MatchedOnOtherTracks);
    assert_eq!(offer.corroborated_titles, 2, "The matched tracks");
    library.origin.shutdown().await;
}

#[tokio::test]
async fn confirming_the_offer_makes_all_four_tracks_one_artist_with_confirmed_provenance() {
    let library = two_of_four().await;
    let credit = unmatched_credit_id(&library).await;
    let receipt = confirm_as(&library, &library.actor, &credit, NOVA)
        .await
        .unwrap();
    assert_eq!(
        (receipt.applied, receipt.kept, receipt.remaining),
        (2, 0, 0)
    );
    let mut groups = Vec::new();
    for recording in &library.recordings {
        groups.push(library.group_of(recording).await.expect("matched"));
    }
    assert!(
        groups.windows(2).all(|pair| pair[0] == pair[1]),
        "{groups:?}"
    );
    assert_eq!(
        library
            .scalar("SELECT count(*) FROM artist_association_candidate_confirmations")
            .await,
        "2"
    );
    assert!(queue(&library).await.is_empty());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn dismissing_the_offer_hides_it_for_good_and_it_cannot_be_confirmed() {
    let library = two_of_four().await;
    let credit = unmatched_credit_id(&library).await;
    let dismissal = dismiss(&library, &credit, Some(NOVA)).await.unwrap();
    assert_eq!(dismissal.dismissed, 1);
    assert!(queue(&library).await.is_empty());
    let hidden = library
        .hito
        .artist_catalog
        .dismissed_artist_reviews(&library.actor, ArtistReviewQueueQuery::default())
        .await
        .unwrap();
    assert_eq!(hidden.items.len(), 1, "{:?}", hidden.items);
    assert_eq!(
        hidden.items[0].candidates[0].origin,
        ArtistReviewOrigin::MatchedOnOtherTracks
    );
    assert!(matches!(
        confirm_as(&library, &library.actor, &credit, NOVA).await,
        Err(Error::NotFound)
    ));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn tracks_matched_by_the_owners_confirmation_are_not_evidence() {
    let mut library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT), (NAMESAKE, CREDIT)]));
    library
        .musicbrainz
        .recordings_of(NOVA, nova_discography(&[("Morning Light", Some(201_000))]));
    library
        .musicbrainz
        .recordings_of(NAMESAKE, nova_discography(&[]));
    library.run().await;
    let credit = unmatched_credit_id(&library).await;
    confirm_as(&library, &library.actor, &credit, NOVA)
        .await
        .unwrap();
    library
        .import_more(
            "later",
            &[(CREDIT, "Late One", 170_000), (CREDIT, "Late Two", 171_000)],
        )
        .await;
    library
        .execute_batch(&format!(
            "DELETE FROM artist_name_candidates;
             INSERT INTO artist_name_checks(user_id,recording_id,observation_id,rule)
             SELECT rc.user_id,r.id,r.metadata_observation_id,5 FROM recordings r
             JOIN account_recording_credits rc ON rc.recording_id=r.id
             WHERE r.id IN ('{}','{}')",
            library.recordings[4], library.recordings[5]
        ))
        .await;
    let items = queue(&library).await;
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].undecided_tracks, 2);
    assert!(items[0].candidates.is_empty() && items[0].needs_search);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn another_account_with_the_same_credit_is_not_offered_the_artist() {
    let library = two_of_four().await;
    let outsider = second_account(&library).await;
    let owner = library.actor.user().id.clone();
    let other = outsider.user().id.clone();
    library
        .execute_batch(&format!(
            "PRAGMA foreign_keys=OFF;
             DROP TRIGGER personal_membership_insert;
             INSERT OR IGNORE INTO library_members(library_id,user_id) SELECT library_id,'{other}' FROM library_members WHERE user_id='{owner}';
             INSERT OR IGNORE INTO account_artists(user_id,id,library_id,name) SELECT '{other}',id,library_id,name FROM account_artists WHERE user_id='{owner}';
             INSERT OR IGNORE INTO account_recording_credits(user_id,recording_id,artist_id) SELECT '{other}',recording_id,artist_id FROM account_recording_credits WHERE user_id='{owner}';
             INSERT INTO artist_name_checks(user_id,recording_id,observation_id,rule)
                SELECT '{other}',recording_id,observation_id,rule FROM artist_name_checks WHERE user_id='{owner}';"
        ))
        .await;
    let theirs = queue_of(&library, &outsider).await;
    assert_eq!(theirs.len(), 1, "{theirs:?}");
    assert!(theirs[0].candidates.is_empty() && theirs[0].needs_search);
    assert!(matches!(
        confirm_as(&library, &outsider, &theirs[0].artist_id, NOVA).await,
        Err(Error::NotFound)
    ));
    assert_eq!(queue(&library).await[0].candidates.len(), 1);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_stored_candidate_with_the_same_identifier_is_merged_into_one_row() {
    let library = two_of_four().await;
    let credit = unmatched_credit_id(&library).await;
    library
        .execute_batch(&format!(
            "INSERT INTO artist_name_candidates(user_id,artist_id,provider_artist_id,position,name,disambiguation,corroborated_titles)
             SELECT user_id,id,'{NOVA}',0,'{CREDIT}','Lisbon fado band',7 FROM account_artists WHERE id='{credit}';
             INSERT INTO artist_name_candidates(user_id,artist_id,provider_artist_id,position,name,disambiguation,corroborated_titles)
             SELECT user_id,id,'{NAMESAKE}',1,'{CREDIT}','Brazilian jazz trio',0 FROM account_artists WHERE id='{credit}';"
        ))
        .await;
    let items = queue(&library).await;
    assert_eq!(items.len(), 1, "{items:?}");
    let offers = &items[0].candidates;
    assert_eq!(offers.len(), 2, "{offers:?}");
    assert_eq!(offers[0].provider_artist_id, NOVA);
    assert_eq!(offers[0].origin, ArtistReviewOrigin::MatchedOnOtherTracks);
    assert_eq!(
        offers[0].disambiguation.as_deref(),
        Some("Lisbon fado band")
    );
    assert_eq!(offers[0].corroborated_titles, 2);
    assert_eq!(offers[1].provider_artist_id, NAMESAKE);
    assert_eq!(offers[1].origin, ArtistReviewOrigin::NameSearch);
    let receipt = confirm_as(&library, &library.actor, &credit, NOVA)
        .await
        .unwrap();
    assert_eq!(receipt.applied, 2);
    library.origin.shutdown().await;
}

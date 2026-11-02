use super::artist_review_matched::{two_of_four, unmatched_credit_id};
use super::artist_review_reach::library::{CREDIT, NAMESAKE, NOVA, NameLibrary};
use super::artist_review_reach::{confirm_as, dismiss, queue, queue_of};
use super::artist_review_same_artist::{edit_artist, second_account};
use hito_core::{ArtistReviewOrigin, ArtistReviewQueueQuery, ConfirmArtistReviewCandidate, Error};
use uuid::Uuid;

async fn offer_is_gone(library: &NameLibrary) {
    let items = queue(library).await;
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].undecided_tracks, 2);
    assert!(
        items[0].candidates.is_empty() && items[0].needs_search,
        "{:?}",
        items[0]
    );
}

async fn break_matched_basis(library: &NameLibrary, sql: &str) {
    library
        .execute_batch(&format!("PRAGMA foreign_keys=OFF; {sql}"))
        .await;
}

#[tokio::test]
async fn a_choice_replaced_by_a_personal_selection_is_not_a_match() {
    let library = two_of_four().await;
    break_matched_basis(
        &library,
        "UPDATE recording_artist_identity_choices SET selection_order=selection_order+100000;",
    )
    .await;
    offer_is_gone(&library).await;
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_retired_choice_is_not_a_match() {
    let library = two_of_four().await;
    break_matched_basis(
        &library,
        "UPDATE recording_artist_identity_choices SET retired=1;",
    )
    .await;
    offer_is_gone(&library).await;
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_choice_for_an_older_observation_is_not_a_match() {
    let library = two_of_four().await;
    break_matched_basis(
        &library,
        "UPDATE recording_artist_identity_choices SET observation_id='older';",
    )
    .await;
    offer_is_gone(&library).await;
    library.origin.shutdown().await;
}

#[tokio::test]
async fn an_unlinked_association_is_not_a_match() {
    let library = two_of_four().await;
    break_matched_basis(
        &library,
        "INSERT INTO recording_artist_association_changes(user_id,recording_id,revision,association_id,linked)
         SELECT user_id,recording_id,1000,association_id,0 FROM recording_artist_identity_choices;",
    )
    .await;
    offer_is_gone(&library).await;
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_hiding_correction_makes_a_match_no_evidence() {
    let library = two_of_four().await;
    let admin = library.actor.user().id.clone();
    break_matched_basis(
        &library,
        &format!(
            "INSERT INTO recording_artist_association_changes(user_id,recording_id,revision,association_id,linked)
             SELECT user_id,recording_id,1000,association_id,1 FROM recording_artist_identity_choices;
             INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_change_id,change_floor) VALUES('c1','{admin}','split','change',1000000);
             INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES((SELECT max(seq) FROM shared_artist_corrections),1,1,'{admin}');
             INSERT INTO shared_artist_correction_subjects(correction_seq,recording_id,observation_id,provider_artist_id,raw_artist)
                SELECT (SELECT max(seq) FROM shared_artist_corrections),r.id,r.metadata_observation_id,'{NOVA}','x'
                FROM recordings r JOIN recording_artist_identity_choices c ON c.recording_id=r.id;"
        ),
    )
    .await;
    offer_is_gone(&library).await;
    library.origin.shutdown().await;
}

#[tokio::test]
async fn tracks_with_a_personal_artist_edit_are_not_matches() {
    let library = two_of_four().await;
    break_matched_basis(
        &library,
        "INSERT INTO personal_recording_metadata(user_id,recording_id,title,artist,override_artist,artist_operation_id)
         SELECT c.user_id,c.recording_id,r.title,r.artist,r.artist,'operation'
         FROM recording_artist_identity_choices c JOIN recordings r ON r.id=c.recording_id;",
    )
    .await;
    let items = queue(&library).await;
    assert!(
        items.iter().all(|item| item
            .candidates
            .iter()
            .all(|offer| offer.origin != ArtistReviewOrigin::MatchedOnOtherTracks)),
        "{items:?}"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_name_match_of_the_weakest_rule_does_not_drive_an_offer() {
    let library = two_of_four().await;
    break_matched_basis(
        &library,
        "DROP TRIGGER artist_association_name_match_immutable;
         UPDATE artist_association_name_matches SET rule=2;",
    )
    .await;
    offer_is_gone(&library).await;
    library.origin.shutdown().await;
}

#[tokio::test]
async fn an_offer_whose_matched_basis_disappears_before_confirming_answers_not_found() {
    let library = two_of_four().await;
    let credit = unmatched_credit_id(&library).await;
    break_matched_basis(
        &library,
        "UPDATE recording_artist_identity_choices SET retired=1;",
    )
    .await;
    assert!(matches!(
        confirm_as(&library, &library.actor, &credit, NOVA).await,
        Err(Error::NotFound)
    ));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn confirming_a_matched_offer_again_with_the_same_request_returns_the_receipt() {
    let library = two_of_four().await;
    let credit = unmatched_credit_id(&library).await;
    let request = ConfirmArtistReviewCandidate {
        request_id: Uuid::new_v4(),
        provider_artist_id: NOVA.into(),
        after: None,
    };
    let catalog = &library.hito.artist_catalog;
    let first = catalog
        .confirm_artist_review_candidate(&library.actor, &credit, request.clone())
        .await
        .unwrap();
    let again = catalog
        .confirm_artist_review_candidate(&library.actor, &credit, request)
        .await
        .unwrap();
    assert_eq!(first, again);
    assert_eq!(first.applied, 2);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn none_of_these_hides_a_matched_offer() {
    let library = two_of_four().await;
    let credit = unmatched_credit_id(&library).await;
    let dismissal = dismiss(&library, &credit, None).await.unwrap();
    assert_eq!(dismissal.dismissed, 1);
    assert!(queue(&library).await.is_empty());
    let hidden = library
        .hito
        .artist_catalog
        .dismissed_artist_reviews(&library.actor, ArtistReviewQueueQuery::default())
        .await
        .unwrap();
    assert_eq!(hidden.items.len(), 1);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_merged_chosen_and_matched_row_confirms_edited_and_undecided_tracks() {
    let mut library = two_of_four().await;
    library
        .import_more("local", &[("Someone Else", "Edited Song", 190_000)])
        .await;
    let edited = library.recordings.last().unwrap().clone();
    edit_artist(&library, &edited, CREDIT).await;
    let items = queue(&library).await;
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].candidates.len(), 1, "{:?}", items[0].candidates);
    assert_eq!(
        items[0].candidates[0].origin,
        ArtistReviewOrigin::ChosenArtist
    );
    let receipt = confirm_as(&library, &library.actor, &items[0].artist_id, NOVA)
        .await
        .unwrap();
    assert_eq!((receipt.applied, receipt.remaining), (3, 0));
    assert!(queue(&library).await.is_empty());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn two_accounts_with_one_credit_name_each_see_only_their_own_matched_artist() {
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
                SELECT '{other}',recording_id,observation_id,rule FROM artist_name_checks WHERE user_id='{owner}';
             INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,provider_artist_id) VALUES('group-other','{other}','builtin_musicbrainz','{NAMESAKE}');
             INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,artist_operation_id,observation_id,retired,selection_order)
                SELECT '{other}',recording_id,revision,association_id,'group-other',artist_id,artist_operation_id,observation_id,retired,selection_order
                FROM recording_artist_identity_choices WHERE user_id='{owner}';"
        ))
        .await;
    let mine = queue(&library).await;
    let theirs = queue_of(&library, &outsider).await;
    assert_eq!(mine[0].candidates.len(), 1, "{mine:?}");
    assert_eq!(mine[0].candidates[0].provider_artist_id, NOVA);
    assert_eq!(theirs.len(), 1, "{theirs:?}");
    assert_eq!(theirs[0].candidates.len(), 1, "{theirs:?}");
    assert_eq!(theirs[0].candidates[0].provider_artist_id, NAMESAKE);
    assert!(matches!(
        confirm_as(&library, &outsider, &theirs[0].artist_id, NOVA).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        confirm_as(&library, &library.actor, &mine[0].artist_id, NAMESAKE).await,
        Err(Error::NotFound)
    ));
    library.origin.shutdown().await;
}

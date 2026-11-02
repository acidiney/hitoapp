use super::artist_review_reach::library::{CREDIT, NOVA, NameLibrary};
use super::artist_review_reach::{EDITED, confirm_as, queue, queue_of};
use super::artist_review_same_artist::{
    chosen_and_edited, edit_artist, edited_credit, second_account,
};
use hito_core::{ArtistReviewOrigin, ArtistReviewQueueQuery, ConfirmArtistReviewCandidate};
use uuid::Uuid;

async fn stored_candidate_beside_a_chosen_artist() -> (NameLibrary, String) {
    let mut library = chosen_and_edited(EDITED).await;
    library
        .import_more("plain", &[(EDITED, "Plain Song", 180_000)])
        .await;
    let plain = library.recordings.last().unwrap().clone();
    library
        .execute(&format!(
            "INSERT INTO artist_name_checks(user_id,recording_id,observation_id,rule)
             SELECT rc.user_id,r.id,r.metadata_observation_id,5 FROM recordings r
             JOIN account_recording_credits rc ON rc.recording_id=r.id WHERE r.id='{plain}'"
        ))
        .await;
    library
        .execute(&format!(
            "INSERT INTO artist_name_candidates(user_id,artist_id,provider_artist_id,position,name,disambiguation,corroborated_titles)
             SELECT user_id,artist_id,'{NOVA}',0,'{CREDIT}','Lisbon fado band',2 FROM account_recording_credits WHERE recording_id='{plain}'"
        ))
        .await;
    (library, plain)
}

#[tokio::test]
async fn a_stored_candidate_and_a_chosen_artist_with_one_identifier_make_one_row() {
    let (library, _) = stored_candidate_beside_a_chosen_artist().await;
    let items = queue(&library).await;
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].undecided_tracks, 2);
    assert_eq!(items[0].candidates.len(), 1, "{:?}", items[0].candidates);
    let candidate = &items[0].candidates[0];
    assert_eq!(candidate.origin, ArtistReviewOrigin::ChosenArtist);
    assert_eq!(
        candidate.disambiguation.as_deref(),
        Some("Lisbon fado band")
    );
    assert_eq!(candidate.corroborated_titles, 2);
    let receipt = confirm_as(&library, &library.actor, &items[0].artist_id, NOVA)
        .await
        .unwrap();
    assert_eq!(receipt.applied, 2, "Both kinds of tracks are reached");
    library.origin.shutdown().await;
}

#[tokio::test]
async fn another_account_that_chose_the_same_name_does_not_make_an_offer() {
    let library = chosen_and_edited(EDITED).await;
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
             INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,plugin_installation_id,provider_artist_id)
                SELECT id||'-o','{other}',provider_kind,plugin_installation_id,provider_artist_id FROM account_artist_identity_groups WHERE user_id='{owner}';
             INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,artist_operation_id,observation_id,retired,selection_order)
                SELECT '{other}',recording_id,revision,association_id,group_id||'-o',artist_id,artist_operation_id,observation_id,retired,selection_order
                FROM recording_artist_identity_choices WHERE user_id='{owner}';
             DELETE FROM recording_artist_identity_choices WHERE user_id='{owner}';"
        ))
        .await;
    let held = library
        .hito
        .artist_catalog
        .artist_identity_groups(&outsider, Default::default())
        .await
        .unwrap();
    assert_eq!(
        held.items.len(),
        1,
        "The other account really holds the chosen artist"
    );
    assert!(
        queue(&library).await.iter().all(|item| item.name != EDITED),
        "Another account's choice is not evidence for this one"
    );
    assert!(queue_of(&library, &outsider).await.is_empty());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_confirmation_replays_and_leaves_the_personal_edit_and_title_untouched() {
    let library = chosen_and_edited(EDITED).await;
    let credit = edited_credit(&library).await;
    let personal = "SELECT group_concat(coalesce(override_artist,'-')||'|'||artist||'|'||title||'|'||coalesce(override_title,'-')||'|'||coalesce(artist_operation_id,'-')||'|'||coalesce(title_operation_id,'-'),';') FROM personal_recording_metadata";
    let before = library.scalar(personal).await;
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
    assert_eq!(first, again, "The stored receipt is replayed");
    assert_eq!(again.applied, 1);
    assert_eq!(library.scalar(personal).await, before);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_confirmation_that_cannot_apply_the_edited_track_dismisses_the_offer() {
    let library = chosen_and_edited(EDITED).await;
    let credit = edited_credit(&library).await;
    let edited = library.recordings.last().unwrap().clone();
    let admin = library.actor.user().id.clone();
    library
        .execute_batch(&format!(
            "PRAGMA foreign_keys=OFF;
             INSERT INTO shared_artist_corrections(id,promoted_by,source_kind,source_change_id,change_floor) VALUES('c1','{admin}','split','change',0);
             INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES((SELECT max(seq) FROM shared_artist_corrections),1,1,'{admin}');
             INSERT INTO shared_artist_correction_subjects(correction_seq,recording_id,observation_id,provider_artist_id,raw_artist)
                SELECT (SELECT max(seq) FROM shared_artist_corrections),r.id,r.metadata_observation_id,'{NOVA}','x' FROM recordings r WHERE r.id='{edited}';"
        ))
        .await;
    let receipt = confirm_as(&library, &library.actor, &credit, NOVA)
        .await
        .unwrap();
    assert_eq!(
        (receipt.applied, receipt.kept, receipt.remaining),
        (0, 1, 0)
    );
    assert!(
        queue(&library).await.is_empty(),
        "The offer does not return"
    );
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
async fn confirming_one_edited_credit_changes_nothing_on_another_credit_with_the_same_offer() {
    let mut library = chosen_and_edited(EDITED).await;
    library
        .import_more("second", &[("Another", "Other Edited Song", 170_000)])
        .await;
    let other = library.recordings.last().unwrap().clone();
    edit_artist(&library, &other, "NOVA QUARTET").await;
    let items = queue(&library).await;
    assert_eq!(items.len(), 2, "{items:?}");
    let first = items.iter().find(|item| item.name == EDITED).unwrap();
    let receipt = confirm_as(&library, &library.actor, &first.artist_id, NOVA)
        .await
        .unwrap();
    assert_eq!(receipt.applied, 1);
    assert!(library.group_of(&other).await.is_none());
    assert_eq!(queue(&library).await.len(), 1);
    library.origin.shutdown().await;
}

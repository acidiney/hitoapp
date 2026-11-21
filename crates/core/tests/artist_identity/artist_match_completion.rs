use super::artist_review_matched::two_of_four;
use super::artist_review_reach::library::{
    CREDIT, NAMESAKE, NOVA, NameLibrary, TRACKS, nova_discography,
};
use super::artist_review_reach::{candidates, confirm_as, dismiss, queue};
use super::artist_review_same_artist::{edit_artist, second_account};
use hito_core::{
    ArtistAssociationEvidence, ArtistCreditRole, AssociateRecordingArtist,
    ChooseRecordingArtistIdentity, DismissArtistMatchSuggestion, MetadataProvider,
};
use uuid::Uuid;

async fn ambiguous(entries: &[(&str, &str, u64)]) -> NameLibrary {
    let library = NameLibrary::prepare(entries).await;
    let name = entries[0].0;
    library
        .musicbrainz
        .search(name, candidates(&[(NOVA, name), (NAMESAKE, name)]));
    library
        .musicbrainz
        .recordings_of(NOVA, nova_discography(&[]));
    library
        .musicbrainz
        .recordings_of(NAMESAKE, nova_discography(&[]));
    library.run().await;
    library
}

async fn ambiguous_nova() -> NameLibrary {
    let entries: Vec<(&str, &str, u64)> = TRACKS
        .iter()
        .map(|(title, length)| (CREDIT, *title, *length))
        .collect();
    ambiguous(&entries).await
}

async fn forget_checks(library: &NameLibrary, indexes: &[usize]) {
    for index in indexes {
        library
            .execute(&format!(
                "DELETE FROM artist_name_checks WHERE recording_id='{}'",
                library.recordings[*index]
            ))
            .await;
    }
}

async fn credit_id(library: &NameLibrary) -> String {
    let items = queue(library).await;
    assert_eq!(items.len(), 1, "{items:?}");
    items[0].artist_id.clone()
}

async fn confirmed_on_first_two() -> (NameLibrary, String) {
    let library = ambiguous_nova().await;
    forget_checks(&library, &[2, 3]).await;
    let credit = credit_id(&library).await;
    library.execute("DELETE FROM artist_matching_jobs").await;
    let receipt = confirm_as(&library, &library.actor, &credit, NOVA)
        .await
        .unwrap();
    assert_eq!((receipt.applied, receipt.remaining), (2, 0));
    (library, credit)
}

async fn choose(library: &NameLibrary, recording: &str, artist: &str) -> String {
    let artist_catalog = &library.hito.artist_catalog;
    let context = artist_catalog
        .begin_artist_review(
            &library.actor,
            recording,
            MetadataProvider::BuiltinMusicBrainz,
        )
        .await
        .unwrap();
    let association = artist_catalog
        .associate_recording_artist(
            &library.actor,
            recording,
            AssociateRecordingArtist {
                artist_id: context.artist_id.clone(),
                source_id: context.source_id.clone(),
                observation_id: context.observation_id.clone(),
                role: ArtistCreditRole::RecordingArtist,
                provider: MetadataProvider::BuiltinMusicBrainz,
                provider_artist_id: artist.into(),
                policy_revision: context.policy_revision,
            },
        )
        .await
        .unwrap();
    let revision = artist_catalog
        .recording_artist_identity_choice(&library.actor, recording)
        .await
        .unwrap()
        .revision;
    artist_catalog
        .choose_recording_artist_identity(
            &library.actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
                association_id: Uuid::parse_str(&association.id).unwrap(),
                context,
            },
        )
        .await
        .unwrap()
        .selection
        .unwrap()
        .group_id
}

async fn completed(library: &NameLibrary, index: usize) -> Option<ArtistAssociationEvidence> {
    let recording = &library.recordings[index];
    library.group_of(recording).await?;
    library
        .associations(recording)
        .await
        .into_iter()
        .find(|association| association.provider_artist_id == NOVA)
        .map(|association| association.evidence)
}

async fn source_tracks(library: &NameLibrary, index: usize) -> String {
    library
        .scalar(&format!(
            "SELECT m.source_tracks FROM artist_association_completion_matches m
             JOIN recording_artist_associations a ON a.id=m.association_id
             WHERE a.recording_id='{}'",
            library.recordings[index]
        ))
        .await
}

async fn undecided(library: &NameLibrary, indexes: &[usize]) {
    undecided_beside(library, indexes, 0).await;
}

async fn undecided_beside(library: &NameLibrary, indexes: &[usize], completions: i64) {
    for index in indexes {
        assert!(
            library
                .group_of(&library.recordings[*index])
                .await
                .is_none(),
            "track {index} stays undecided"
        );
    }
    assert_eq!(
        library.rows("artist_association_completion_matches").await,
        completions
    );
}

#[tokio::test]
async fn confirming_an_artist_for_some_tracks_completes_the_credits_other_tracks() {
    let (library, _) = confirmed_on_first_two().await;
    let group = library.group_of(&library.recordings[0]).await.unwrap();
    for index in [2, 3] {
        assert_eq!(
            library.group_of(&library.recordings[index]).await.as_ref(),
            Some(&group)
        );
        assert_eq!(
            completed(&library, index).await,
            Some(ArtistAssociationEvidence::MatchedCreditCompletion)
        );
        assert_eq!(source_tracks(&library, index).await, "2");
    }
    for index in [0, 1] {
        assert_eq!(
            completed(&library, index).await,
            Some(ArtistAssociationEvidence::UserConfirmedCandidate)
        );
    }
    assert_eq!(
        library.rows("artist_association_completion_matches").await,
        2
    );
    assert_eq!(library.automatic().await, 0, "Nothing is left to do");
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_hand_choice_for_one_track_completes_the_credits_other_tracks() {
    let library = ambiguous_nova().await;
    library.execute("DELETE FROM artist_matching_jobs").await;
    let group = choose(&library, &library.recordings[0], NOVA).await;
    assert_eq!(
        library.rows("artist_matching_jobs").await,
        0,
        "The choice completes its credit at once instead of starting a walk"
    );
    assert_eq!(
        completed(&library, 0).await,
        Some(ArtistAssociationEvidence::UserConfirmed)
    );
    for index in [1, 2, 3] {
        assert_eq!(
            library.group_of(&library.recordings[index]).await,
            Some(group.clone())
        );
        assert_eq!(
            completed(&library, index).await,
            Some(ArtistAssociationEvidence::MatchedCreditCompletion)
        );
        assert_eq!(source_tracks(&library, index).await, "1");
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_track_with_a_personal_artist_edit_is_left_alone() {
    let library = ambiguous_nova().await;
    forget_checks(&library, &[2, 3]).await;
    edit_artist(&library, &library.recordings[3], CREDIT).await;
    let credit = credit_id(&library).await;
    confirm_as(&library, &library.actor, &credit, NOVA)
        .await
        .unwrap();
    library.automatic().await;
    assert_eq!(
        completed(&library, 2).await,
        Some(ArtistAssociationEvidence::MatchedCreditCompletion)
    );
    let edited = &library.recordings[3];
    assert!(library.group_of(edited).await.is_none());
    assert!(
        library
            .associations(edited)
            .await
            .iter()
            .all(|association| association.provider_artist_id != NOVA)
    );
    library.origin.shutdown().await;
}

async fn retire(library: &NameLibrary, indexes: &[usize]) {
    for index in indexes {
        library
            .execute(&format!(
                "UPDATE recording_artist_identity_choices SET retired=1,revision=revision+1
                 WHERE retired=0 AND recording_id='{}'",
                library.recordings[*index]
            ))
            .await;
    }
}

#[tokio::test]
async fn a_current_choice_for_another_artist_on_the_credit_stops_completion() {
    let (personal, _) = confirmed_on_first_two().await;
    choose(&personal, &personal.recordings[2], NAMESAKE).await;
    retire(&personal, &[3]).await;
    personal.automatic().await;
    undecided_beside(&personal, &[3], 2).await;
    personal.origin.shutdown().await;

    let automatic = two_of_four().await;
    choose(&automatic, &automatic.recordings[2], NAMESAKE).await;
    automatic.automatic().await;
    undecided(&automatic, &[3]).await;
    automatic.origin.shutdown().await;
}

#[tokio::test]
async fn an_artist_dismissed_for_the_credit_is_not_completed() {
    let (reviewed, credit) = confirmed_on_first_two().await;
    dismiss(&reviewed, &credit, Some(NOVA)).await.unwrap();
    retire(&reviewed, &[2, 3]).await;
    reviewed.automatic().await;
    undecided_beside(&reviewed, &[2, 3], 2).await;
    reviewed.origin.shutdown().await;

    let (suggested, credit) = confirmed_on_first_two().await;
    let group = suggested.group_of(&suggested.recordings[0]).await.unwrap();
    suggested
        .hito
        .artist_catalog
        .dismiss_artist_match_suggestion(
            &suggested.actor,
            &credit,
            DismissArtistMatchSuggestion { group_id: group },
        )
        .await
        .unwrap();
    retire(&suggested, &[2, 3]).await;
    suggested.automatic().await;
    undecided_beside(&suggested, &[2, 3], 2).await;
    suggested.origin.shutdown().await;
}

#[tokio::test]
async fn another_account_with_the_same_credit_gets_nothing() {
    let (library, _) = confirmed_on_first_two().await;
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
             INSERT INTO artist_matching_jobs(user_id,next_run_at) VALUES('{other}',0);"
        ))
        .await;
    library.automatic().await;
    assert_eq!(
        completed(&library, 3).await,
        Some(ArtistAssociationEvidence::MatchedCreditCompletion)
    );
    assert_eq!(
        library
            .scalar(&format!(
                "SELECT count(*) FROM recording_artist_identity_choices WHERE user_id='{other}'"
            ))
            .await,
        "0"
    );
    assert_eq!(
        library
            .scalar(&format!(
                "SELECT count(*) FROM recording_artist_associations WHERE user_id='{other}'"
            ))
            .await,
        "0"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_credit_that_lists_several_artists_completes_nothing() {
    let name = "Nova & Friends";
    let library = ambiguous(&[
        (name, "Morning Light", 201_000),
        (name, "Second Wind", 187_500),
        (name, "Harbor Lights", 243_000),
    ])
    .await;
    choose(&library, &library.recordings[0], NOVA).await;
    library.automatic().await;
    undecided(&library, &[1, 2]).await;
    library.origin.shutdown().await;
}

#[tokio::test]
async fn weak_name_matches_alone_complete_nothing() {
    let library = two_of_four().await;
    library.automatic().await;
    undecided(&library, &[2, 3]).await;
    library.origin.shutdown().await;
}

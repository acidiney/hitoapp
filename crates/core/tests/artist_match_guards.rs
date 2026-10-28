#[allow(dead_code)]
#[path = "fixtures/artist_match_library.rs"]
mod library;

use hito_core::test_support::support;
use hito_core::{
    Action, Actor, PromoteArtistCorrection, PromotionSource, RecordingArtistIdentityState,
    ResetRecordingArtistIdentity, SplitArtistIdentityGroup, SplitDestination,
    UnlinkRecordingArtist,
};
use library::{CREDIT, Library, M1, M2, tagged};
use uuid::Uuid;

async fn group_of(library: &Library, actor: &Actor, recording: &str) -> Option<String> {
    match library
        .hito
        .catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap()
        .state
    {
        RecordingArtistIdentityState::Selected(selection) => Some(selection.group_id),
        _ => None,
    }
}

#[tokio::test]
async fn a_shared_correction_is_not_undone_by_another_accounts_pass() {
    let library = Library::prepare().await;
    library.run().await;
    let association = library
        .hito
        .catalog
        .recording_artist_associations(&library.actor, &library.a)
        .await
        .unwrap()
        .remove(0);
    let revision = library
        .hito
        .catalog
        .recording_artist_association_changes(&library.actor, &library.a)
        .await
        .unwrap()
        .revision;
    library
        .hito
        .catalog
        .unlink_recording_artist(
            &library.actor,
            &library.a,
            UnlinkRecordingArtist {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
                association_id: Uuid::parse_str(&association.id).unwrap(),
            },
        )
        .await
        .unwrap();
    library
        .hito
        .catalog
        .promote_artist_correction(
            &library.actor,
            PromoteArtistCorrection {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
                source: PromotionSource::Unlink {
                    recording_id: library.a.clone(),
                    association_id: Uuid::parse_str(&association.id).unwrap(),
                },
            },
        )
        .await
        .unwrap();

    let peer = library.run_as(&library.peer).await;
    assert_eq!(
        (peer.matched, peer.skipped_declined),
        (2, 1),
        "The administrator's shared correction holds for an account that never reviewed the artist"
    );
    assert!(
        group_of(&library, &library.peer, &library.a)
            .await
            .is_none()
    );
    assert!(
        library
            .hito
            .catalog
            .recording_artist_associations(&library.peer, &library.a)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        group_of(&library, &library.peer, &library.b)
            .await
            .is_some()
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_retagged_automatic_match_is_matched_again_but_a_user_decision_is_not() {
    let library = Library::prepare().await;
    library.run().await;
    let choice = library
        .hito
        .catalog
        .recording_artist_identity_choice(&library.actor, &library.b)
        .await
        .unwrap();
    library
        .hito
        .catalog
        .reset_recording_artist_identity(
            &library.actor,
            &library.b,
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
            },
        )
        .await
        .unwrap();
    tagged(
        &library.music.join("a.wav"),
        1,
        ("Song A", "Album A"),
        CREDIT,
        Some(M2),
    );
    tagged(
        &library.music.join("b.wav"),
        2,
        ("Song B", "Album B"),
        library::SPELLED,
        Some(M2),
    );
    support::scan(&library.hito, &library.actor, &library.library).await;
    let again = library.run().await;
    assert_eq!(
        again.matched, 1,
        "Only the untouched automatic match is re-evaluated"
    );
    let second = group_of(&library, &library.actor, &library.namesake)
        .await
        .unwrap();
    assert_eq!(
        group_of(&library, &library.actor, &library.a).await,
        Some(second),
        "The retagged recording follows its new embedded identifier"
    );
    assert!(
        group_of(&library, &library.actor, &library.b)
            .await
            .is_none(),
        "A recording the account reset itself stays decided"
    );
    let _ = M1;
    library.origin.shutdown().await;
}

#[tokio::test]
async fn personal_edits_splits_and_retired_choices_are_left_alone() {
    let library = Library::prepare().await;
    support::synchronize(
        &library.hito,
        &library.actor,
        vec![Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(&library.a).unwrap(),
            artist: "Personally edited".into(),
        }],
    )
    .await;
    let first = library.run().await;
    assert_eq!((first.matched, first.skipped_declined), (2, 1));
    assert!(
        group_of(&library, &library.actor, &library.a)
            .await
            .is_none()
    );

    let group = group_of(&library, &library.actor, &library.b)
        .await
        .unwrap();
    library
        .hito
        .catalog
        .split_artist_identity_group(
            &library.actor,
            &group,
            SplitArtistIdentityGroup {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
                recordings: vec![Uuid::parse_str(&library.b).unwrap()],
                releases: vec![],
                destination: SplitDestination::Credit,
            },
        )
        .await
        .unwrap();
    support::synchronize(
        &library.hito,
        &library.actor,
        vec![Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(&library.namesake).unwrap(),
            artist: "Edited after the match".into(),
        }],
    )
    .await;
    let second = library.run().await;
    assert_eq!(second.matched, 0);
    for recording in [&library.a, &library.b, &library.namesake] {
        assert!(
            group_of(&library, &library.actor, recording)
                .await
                .is_none(),
            "an edited, split or retired recording is not matched again"
        );
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn concurrent_passes_match_each_recording_once() {
    let library = Library::prepare().await;
    let (left, right) = tokio::join!(library.run(), library.run());
    assert_eq!(left.matched + right.matched, 3);
    assert_eq!(library.rows("artist_association_matches").await, 3);
    assert_eq!(library.rows("recording_artist_identity_choices").await, 3);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn declined_and_ambiguous_recordings_are_checked_once_per_observation() {
    let library = Library::prepare().await;
    let first = library.run().await;
    assert_eq!((first.matched, first.skipped_ambiguous), (3, 1));
    let checked = library.rows("artist_match_checks").await;
    assert_eq!(checked, 1, "The ambiguous file is remembered");
    let second = library.run().await;
    assert_eq!(
        (
            second.matched,
            second.skipped_ambiguous,
            second.skipped_declined
        ),
        (0, 0, 0),
        "A second pass does no rework"
    );
    assert_eq!(library.rows("artist_match_checks").await, checked);
    library.origin.shutdown().await;
}

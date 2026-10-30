use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[allow(dead_code)]
#[path = "../fixtures/artist_identity_selection_library.rs"]
mod library;
#[allow(unused_imports)]
use hito_core::test_support::support;

use futures_util::{FutureExt, future::BoxFuture};
use hito_core::{
    Actor, ApplyArtistIdentityToCredit, ArtistIdentityChangeKind, ArtistIdentityChangeReceipt,
    Browse, ChooseRecordingArtistIdentity, Error, RecordingArtistIdentityChoice,
    RecordingArtistIdentityState, ResetRecordingArtistIdentity, SplitArtistIdentityGroup,
    SplitDestination, UndoArtistIdentityChange,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

async fn run(exercise: for<'a> fn(&'a library::Library) -> BoxFuture<'a, ()>) {
    let mut library = library::Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.origin.shutdown().await;
    library.catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn review(library: &library::Library, recording: &str, identity: &str) -> String {
    let artist_catalog = &library.hito.artist_catalog;
    let context = artist_catalog
        .begin_artist_review(&library.actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    artist_catalog
        .confirm_artist_review(&library.actor, &context, identity)
        .await
        .unwrap()
        .id
}

async fn choose(library: &library::Library, recording: &str, identity: &str) -> String {
    let artist_catalog = &library.hito.artist_catalog;
    let context = artist_catalog
        .begin_artist_review(&library.actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    let association = artist_catalog
        .confirm_artist_review(&library.actor, &context, identity)
        .await
        .unwrap();
    let choice = choice(library, &library.actor, recording).await;
    artist_catalog
        .choose_recording_artist_identity(
            &library.actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
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

async fn choice(
    library: &library::Library,
    actor: &Actor,
    recording: &str,
) -> RecordingArtistIdentityChoice {
    library
        .hito
        .artist_catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap()
}

fn group_of(choice: &RecordingArtistIdentityChoice) -> Option<&str> {
    match &choice.state {
        RecordingArtistIdentityState::Selected(selection) => Some(&selection.group_id),
        _ => None,
    }
}

async fn merged(library: &library::Library) -> String {
    let alpha = choose(library, &library.local, catalog::ALPHA).await;
    let credit = credit_of_imported(library).await;
    library
        .hito
        .artist_catalog
        .apply_artist_identity_to_credit(
            &library.actor,
            &credit,
            ApplyArtistIdentityToCredit {
                request_id: Uuid::new_v4(),
                group_id: alpha.clone(),
                after: None,
                limit: None,
            },
        )
        .await
        .unwrap();
    alpha
}

async fn credit_of_imported(library: &library::Library) -> String {
    credits(library)
        .await
        .into_iter()
        .find(|artist| artist.library_id != library.shared)
        .unwrap()
        .id
}

async fn credits(library: &library::Library) -> Vec<hito_core::Artist> {
    library
        .hito
        .catalog
        .artists(
            &library.actor,
            Browse {
                q: Some(catalog::CREDIT.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .items
}

fn request(
    revision: i64,
    recordings: &[&str],
    destination: SplitDestination,
) -> SplitArtistIdentityGroup {
    SplitArtistIdentityGroup {
        request_id: Uuid::new_v4(),
        expected_revision: revision,
        recordings: recordings
            .iter()
            .map(|id| Uuid::parse_str(id).unwrap())
            .collect(),
        releases: Vec::new(),
        destination,
    }
}

fn beta(library: &library::Library) -> SplitDestination {
    SplitDestination::Identity {
        provider: library.metadata_provider.clone(),
        provider_artist_id: catalog::BETA.into(),
    }
}

async fn split(
    library: &library::Library,
    actor: &Actor,
    group: &str,
    request: SplitArtistIdentityGroup,
) -> Result<ArtistIdentityChangeReceipt, Error> {
    library
        .hito
        .artist_catalog
        .split_artist_identity_group(actor, group, request)
        .await
}

async fn undo(
    library: &library::Library,
    group: &str,
    revision: i64,
) -> Result<ArtistIdentityChangeReceipt, Error> {
    library
        .hito
        .artist_catalog
        .undo_artist_identity_change(
            &library.actor,
            group,
            UndoArtistIdentityChange {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
            },
        )
        .await
}

async fn members(library: &library::Library, group: &str) -> i64 {
    library
        .hito
        .artist_catalog
        .artist_identity_group(&library.actor, group, Default::default())
        .await
        .unwrap()
        .group
        .recording_count
}

#[tokio::test]
async fn a_split_moves_chosen_recordings_to_a_reviewed_artist_and_undo_restores_them() {
    run(|library| {
        Box::pin(async move {
            let alpha = merged(library).await;
            review(library, &library.namesake, catalog::BETA).await;
            assert_eq!(members(library, &alpha).await, 3);
            let before = library.snapshot().await;
            let origin = library.origin_requests().await;
            let restored = choice(library, &library.actor, &library.namesake).await;

            let moved = split(
                library,
                &library.actor,
                &alpha,
                request(0, &[&library.namesake], beta(library)),
            )
            .await
            .unwrap();
            assert_eq!(
                (moved.revision, moved.recordings, moved.kind),
                (1, 1, ArtistIdentityChangeKind::Split)
            );
            let beta_group = moved.destination_group_id.clone().unwrap();
            let current = choice(library, &library.actor, &library.namesake).await;
            assert_eq!(group_of(&current), Some(beta_group.as_str()));
            assert_eq!(current.revision, restored.revision + 1);
            assert_eq!(members(library, &alpha).await, 2);
            assert_eq!(members(library, &beta_group).await, 1);
            assert_eq!(
                library
                    .hito
                    .artist_catalog
                    .artist_identity_groups(&library.actor, Default::default())
                    .await
                    .unwrap()
                    .total,
                2
            );
            assert_eq!(library.origin_requests().await, origin);
            assert_eq!(
                library.snapshot().await,
                before,
                "Recordings, sources, likes, progress and personal edits are unchanged"
            );

            let state = library
                .hito
                .artist_catalog
                .artist_identity_group_changes(&library.actor, &alpha)
                .await
                .unwrap();
            let latest = state.latest.unwrap();
            assert_eq!(state.revision, 1);
            assert_eq!((latest.recordings, latest.retired), (1, 0));
            assert!(latest.undoable && !latest.shareable && latest.shared.is_none());

            let replay = SplitArtistIdentityGroup {
                request_id: Uuid::new_v4(),
                ..request(1, &[&library.local], SplitDestination::Credit)
            };
            let again = split(library, &library.actor, &alpha, replay.clone())
                .await
                .unwrap();
            assert_eq!(
                split(library, &library.actor, &alpha, replay)
                    .await
                    .unwrap(),
                again,
                "An exact replay returns the stored receipt"
            );
            assert!(matches!(
                split(
                    library,
                    &library.actor,
                    &alpha,
                    SplitArtistIdentityGroup {
                        request_id: again.request_id,
                        ..request(1, &[&library.imported], SplitDestination::Credit)
                    }
                )
                .await,
                Err(Error::Conflict(_))
            ));
            assert_eq!(again.revision, 2);
            assert_eq!(
                group_of(&choice(library, &library.actor, &library.local).await),
                None,
                "Splitting to the library credit leaves the recording unselected"
            );
            assert!(
                credits(library)
                    .await
                    .iter()
                    .any(|credit| credit.library_id == library.shared),
                "The credit returns to the Library credits list"
            );

            let undone = undo(library, &alpha, 2).await.unwrap();
            assert_eq!(
                (undone.revision, undone.kind, undone.recordings),
                (3, ArtistIdentityChangeKind::Undo, 1)
            );
            assert_eq!(
                group_of(&choice(library, &library.actor, &library.local).await),
                Some(alpha.as_str())
            );
            let repeated = undo(library, &alpha, 3).await.unwrap();
            assert_eq!(repeated.revision, 4, "Undoing the undo repeats the split");
            assert_eq!(
                group_of(&choice(library, &library.actor, &library.local).await),
                None
            );
            undo(library, &alpha, 4).await.unwrap();
            assert!(matches!(
                undo(library, &alpha, 4).await,
                Err(Error::Conflict(_))
            ));
            assert!(matches!(
                undo(library, &alpha, 9).await,
                Err(Error::Conflict(_))
            ));
        })
    })
    .await;
}

#[tokio::test]
async fn undoing_the_only_split_restores_the_exact_choice_state() {
    run(|library| {
        Box::pin(async move {
            let alpha = merged(library).await;
            review(library, &library.namesake, catalog::BETA).await;
            let before = library.snapshot().await;
            let original = choice(library, &library.actor, &library.namesake).await;
            let moved = split(
                library,
                &library.actor,
                &alpha,
                request(0, &[&library.namesake], beta(library)),
            )
            .await
            .unwrap();
            undo(library, &alpha, moved.revision).await.unwrap();
            let current = choice(library, &library.actor, &library.namesake).await;
            assert_eq!(group_of(&current), group_of(&original));
            assert_eq!(current.revision, original.revision + 2);
            assert_eq!(members(library, &alpha).await, 3);
            assert_eq!(library.snapshot().await, before);
        })
    })
    .await;
}

#[tokio::test]
async fn selecting_releases_moves_only_the_recordings_that_appear_on_them() {
    run(|library| {
        Box::pin(async move {
            let alpha = merged(library).await;
            let detail = library
                .hito
                .artist_catalog
                .artist_identity_group(&library.actor, &alpha, Default::default())
                .await
                .unwrap();
            let releases = detail
                .releases
                .items
                .iter()
                .map(|release| Uuid::parse_str(&release.id).unwrap())
                .collect::<Vec<_>>();
            assert!(!releases.is_empty());
            let moved = split(
                library,
                &library.actor,
                &alpha,
                SplitArtistIdentityGroup {
                    releases,
                    ..request(0, &[], SplitDestination::Credit)
                },
            )
            .await
            .unwrap();
            let remaining = members(library, &alpha).await;
            assert_eq!(moved.recordings + remaining, 3);
            assert!(
                moved.recordings >= 1 && remaining >= 1,
                "Recordings that appear on no selected release stay with the artist"
            );
        })
    })
    .await;
}

#[tokio::test]
async fn invalid_conflicting_and_foreign_splits_change_nothing() {
    run(|library| {
        Box::pin(async move {
            let alpha = merged(library).await;
            review(library, &library.namesake, catalog::BETA).await;
            let before = library.snapshot().await;
            assert!(matches!(
                split(
                    library,
                    &library.actor,
                    &alpha,
                    request(0, &[], SplitDestination::Credit)
                )
                .await,
                Err(Error::Invalid(_))
            ));
            assert!(
                matches!(
                    split(
                        library,
                        &library.actor,
                        &alpha,
                        request(0, &[&library.local, &library.namesake], beta(library))
                    )
                    .await,
                    Err(Error::Conflict(_))
                ),
                "A recording without a review of the destination blocks the whole split"
            );
            assert!(matches!(
                split(
                    library,
                    &library.actor,
                    &alpha,
                    request(0, &[&Uuid::new_v4().to_string()], SplitDestination::Credit)
                )
                .await,
                Err(Error::Conflict(_))
            ));
            assert!(matches!(
                split(
                    library,
                    &library.actor,
                    &alpha,
                    request(
                        0,
                        &[&library.local],
                        SplitDestination::Identity {
                            provider: library.metadata_provider.clone(),
                            provider_artist_id: catalog::ALPHA.into()
                        }
                    )
                )
                .await,
                Err(Error::Invalid(_))
            ));
            assert!(matches!(
                split(
                    library,
                    &library.actor,
                    &alpha,
                    request(5, &[&library.local], SplitDestination::Credit)
                )
                .await,
                Err(Error::Conflict(_))
            ));
            assert!(matches!(
                split(
                    library,
                    &library.peer,
                    &alpha,
                    request(0, &[&library.local], SplitDestination::Credit)
                )
                .await,
                Err(Error::NotFound)
            ));
            assert!(matches!(
                split(
                    library,
                    &library.actor,
                    "missing",
                    request(0, &[&library.local], SplitDestination::Credit)
                )
                .await,
                Err(Error::NotFound)
            ));
            assert_eq!(members(library, &alpha).await, 3);
            assert_eq!(
                library
                    .hito
                    .artist_catalog
                    .artist_identity_group_changes(&library.actor, &alpha)
                    .await
                    .unwrap()
                    .revision,
                0
            );
            assert_eq!(library.snapshot().await, before);

            let moved = split(
                library,
                &library.actor,
                &alpha,
                request(0, &[&library.namesake], beta(library)),
            )
            .await
            .unwrap();
            let current = choice(library, &library.actor, &library.namesake).await;
            library
                .hito
                .artist_catalog
                .reset_recording_artist_identity(
                    &library.actor,
                    &library.namesake,
                    ResetRecordingArtistIdentity {
                        request_id: Uuid::new_v4(),
                        expected_revision: current.revision,
                    },
                )
                .await
                .unwrap();
            assert!(
                matches!(
                    undo(library, &alpha, moved.revision).await,
                    Err(Error::Conflict(_))
                ),
                "A recording that changed after the split blocks the undo"
            );
            assert!(matches!(
                library
                    .hito
                    .artist_catalog
                    .artist_identity_group_changes(&library.peer, &alpha)
                    .await,
                Err(Error::NotFound)
            ));
        })
    })
    .await;
}

#[tokio::test]
async fn undo_is_refused_when_an_association_it_would_restore_was_unlinked_since() {
    run(|library| {
        Box::pin(async move {
            let alpha = merged(library).await;
            review(library, &library.namesake, catalog::BETA).await;
            let associations = library
                .hito
                .artist_catalog
                .recording_artist_associations(&library.actor, &library.namesake)
                .await
                .unwrap();
            let original = associations
                .iter()
                .find(|association| association.provider_artist_id == catalog::ALPHA)
                .unwrap()
                .id
                .clone();
            let moved = split(
                library,
                &library.actor,
                &alpha,
                request(0, &[&library.namesake], beta(library)),
            )
            .await
            .unwrap();
            library
                .hito
                .artist_catalog
                .unlink_recording_artist(
                    &library.actor,
                    &library.namesake,
                    hito_core::UnlinkRecordingArtist {
                        request_id: Uuid::new_v4(),
                        expected_revision: library
                            .hito
                            .artist_catalog
                            .recording_artist_association_changes(&library.actor, &library.namesake)
                            .await
                            .unwrap()
                            .revision,
                        association_id: Uuid::parse_str(&original).unwrap(),
                    },
                )
                .await
                .unwrap();
            assert!(
                matches!(
                    undo(library, &alpha, moved.revision).await,
                    Err(Error::Conflict(_))
                ),
                "Undo must not restore a choice that points at an unlinked review"
            );
            assert_eq!(members(library, &alpha).await, 2);
        })
    })
    .await;
}

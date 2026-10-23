use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[allow(dead_code)]
#[path = "fixtures/artist_identity_selection_library.rs"]
mod library;
use futures_util::{FutureExt, future::BoxFuture};
#[allow(unused_imports)]
use hito_core::test_support::support;
use hito_core::{
    Action, Actor, ApplyArtistIdentityToCredit, Artist, ArtistIdentityCreditReceipt, Browse,
    ChooseRecordingArtistIdentity, Error, RecordingArtistIdentityChoice,
    RecordingArtistIdentityState, ResetRecordingArtistIdentity,
};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use sqlx::sqlite::SqliteConnectOptions;
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

async fn credits(library: &library::Library) -> Vec<Artist> {
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

async fn choose(library: &library::Library, recording: &str, identity: &str) -> String {
    let catalog = &library.hito.catalog;
    let context = catalog
        .begin_artist_review(&library.actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    let association = catalog
        .confirm_artist_review(&library.actor, &context, identity)
        .await
        .unwrap();
    let choice = catalog
        .recording_artist_identity_choice(&library.actor, recording)
        .await
        .unwrap();
    catalog
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
        .catalog
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

async fn apply(
    library: &library::Library,
    actor: &Actor,
    credit: &str,
    request: ApplyArtistIdentityToCredit,
) -> Result<ArtistIdentityCreditReceipt, Error> {
    library
        .hito
        .catalog
        .apply_artist_identity_to_credit(actor, credit, request)
        .await
}

fn request(group: &str) -> ApplyArtistIdentityToCredit {
    ApplyArtistIdentityToCredit {
        request_id: Uuid::new_v4(),
        group_id: group.into(),
        after: None,
        limit: None,
    }
}

async fn imported_credit(library: &library::Library) -> String {
    credits(library)
        .await
        .into_iter()
        .find(|artist| artist.library_id != library.shared)
        .unwrap()
        .id
}

#[tokio::test]
async fn one_reviewed_identity_covers_every_accessible_recording_under_the_credit() {
    run(|library| {
        Box::pin(async move {
            let alpha = choose(library, &library.local, catalog::ALPHA).await;
            let credit = imported_credit(library).await;
            let before = library.snapshot().await;
            let origin = library.origin_requests().await;
            let applied = request(&alpha);
            let first = apply(
                library,
                &library.actor,
                &credit,
                ApplyArtistIdentityToCredit {
                    request_id: applied.request_id,
                    group_id: applied.group_id.clone(),
                    after: None,
                    limit: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(
                (
                    first.applied,
                    first.unchanged,
                    first.skipped_different,
                    first.skipped_unavailable
                ),
                (2, 0, 0, 0)
            );
            assert!(first.next_after.is_none());
            for recording in [&library.imported, &library.namesake] {
                let current = choice(library, &library.actor, recording).await;
                assert_eq!(group_of(&current), Some(alpha.as_str()));
                assert_eq!(current.revision, 1);
            }
            assert_eq!(
                choice(library, &library.actor, &library.local)
                    .await
                    .revision,
                1
            );
            assert_eq!(
                group_of(&choice(library, &library.peer, &library.local).await),
                None,
                "Another account's recording choice is unaffected"
            );
            assert!(
                credits(library).await.is_empty(),
                "Both credits are covered by the chosen identity and leave the index"
            );
            let groups = library
                .hito
                .catalog
                .artist_identity_groups(&library.actor, Default::default())
                .await
                .unwrap();
            assert_eq!((groups.total, groups.items[0].recording_count), (1, 3));

            let replay = apply(
                library,
                &library.actor,
                &credit,
                ApplyArtistIdentityToCredit {
                    request_id: applied.request_id,
                    group_id: alpha.clone(),
                    after: None,
                    limit: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(replay, first, "The same operation returns the same receipt");
            assert_eq!(
                choice(library, &library.actor, &library.imported)
                    .await
                    .revision,
                1
            );
            assert!(matches!(
                apply(
                    library,
                    &library.actor,
                    &credit,
                    ApplyArtistIdentityToCredit {
                        request_id: applied.request_id,
                        group_id: alpha.clone(),
                        after: None,
                        limit: Some(5),
                    }
                )
                .await,
                Err(Error::Conflict(_))
            ));

            let reset = choice(library, &library.actor, &library.imported).await;
            library
                .hito
                .catalog
                .reset_recording_artist_identity(
                    &library.actor,
                    &library.imported,
                    ResetRecordingArtistIdentity {
                        request_id: Uuid::new_v4(),
                        expected_revision: reset.revision,
                    },
                )
                .await
                .unwrap();
            assert_eq!(
                group_of(&choice(library, &library.actor, &library.imported).await),
                None
            );
            assert_eq!(
                credits(library).await.len(),
                1,
                "Resetting one recording returns its credit to the index"
            );
            assert!(matches!(
                apply(library, &library.peer, &credit, request(&alpha)).await,
                Err(Error::NotFound)
            ));
            assert_eq!(library.origin_requests().await, origin);
            assert_eq!(
                library.snapshot().await,
                before,
                "Recordings, sources and personal state are unchanged"
            );
        })
    })
    .await;
}

#[tokio::test]
async fn existing_different_choices_are_skipped_and_counted_without_overwriting() {
    run(|library| {
        Box::pin(async move {
            let alpha = choose(library, &library.local, catalog::ALPHA).await;
            let beta = choose(library, &library.namesake, catalog::BETA).await;
            assert_ne!(alpha, beta);
            let credit = imported_credit(library).await;
            let receipt = apply(library, &library.actor, &credit, request(&alpha))
                .await
                .unwrap();
            assert_eq!(
                (
                    receipt.applied,
                    receipt.unchanged,
                    receipt.skipped_different,
                    receipt.skipped_unavailable
                ),
                (1, 0, 1, 0)
            );
            let kept = choice(library, &library.actor, &library.namesake).await;
            assert_eq!((group_of(&kept), kept.revision), (Some(beta.as_str()), 1));
            assert_eq!(
                group_of(&choice(library, &library.actor, &library.imported).await),
                Some(alpha.as_str())
            );
            let again = apply(library, &library.actor, &credit, request(&alpha))
                .await
                .unwrap();
            assert_eq!(
                (again.applied, again.unchanged, again.skipped_different),
                (0, 1, 1)
            );
            assert_eq!(
                choice(library, &library.actor, &library.imported)
                    .await
                    .revision,
                1
            );
            assert!(matches!(
                apply(library, &library.actor, &credit, request("missing-group")).await,
                Err(Error::NotFound)
            ));
            assert!(matches!(
                apply(library, &library.actor, "missing-credit", request(&alpha)).await,
                Err(Error::NotFound)
            ));
        })
    })
    .await;
}

#[tokio::test]
async fn bounded_batches_resume_from_a_cursor_and_need_a_current_reviewed_member() {
    run(|library| {
        Box::pin(async move {
            let alpha = choose(library, &library.local, catalog::ALPHA).await;
            let credit = imported_credit(library).await;
            let first = apply(
                library,
                &library.actor,
                &credit,
                ApplyArtistIdentityToCredit {
                    limit: Some(1),
                    ..request(&alpha)
                },
            )
            .await
            .unwrap();
            assert_eq!(first.applied, 1);
            let cursor = first.next_after.clone().expect("More recordings remain");
            let second = apply(
                library,
                &library.actor,
                &credit,
                ApplyArtistIdentityToCredit {
                    limit: Some(1),
                    after: Some(cursor),
                    ..request(&alpha)
                },
            )
            .await
            .unwrap();
            assert_eq!((second.applied, second.next_after), (1, None));
            for limit in [0, 101, -1] {
                assert!(matches!(
                    apply(
                        library,
                        &library.actor,
                        &credit,
                        ApplyArtistIdentityToCredit {
                            limit: Some(limit),
                            ..request(&alpha)
                        }
                    )
                    .await,
                    Err(Error::Invalid(_))
                ));
            }
            for recording in [&library.local, &library.imported, &library.namesake] {
                let current = choice(library, &library.actor, recording).await;
                library
                    .hito
                    .catalog
                    .reset_recording_artist_identity(
                        &library.actor,
                        recording,
                        ResetRecordingArtistIdentity {
                            request_id: Uuid::new_v4(),
                            expected_revision: current.revision,
                        },
                    )
                    .await
                    .unwrap();
            }
            assert!(
                matches!(
                    apply(library, &library.actor, &credit, request(&alpha)).await,
                    Err(Error::Conflict(_))
                ),
                "A group with no current reviewed recording is not accepted as evidence"
            );
        })
    })
    .await;
}

const OTHER: &str = "Other Performer";

async fn retire(library: &library::Library, recordings: &[&String]) {
    let actions = recordings
        .iter()
        .map(|recording| Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(recording).unwrap(),
            artist: catalog::CREDIT.into(),
        })
        .collect();
    support::synchronize(&library.hito, &library.actor, actions).await;
}

#[tokio::test]
async fn a_group_reviewed_only_for_another_credit_name_is_not_evidence_and_writes_nothing() {
    run(|library| {
        Box::pin(async move {
            let path = library.music().join("other.wav");
            support::write_wav(&path, 1);
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title("Other performer track".into());
            tag.set_artist(OTHER.into());
            tag.save_to_path(&path, Default::default()).unwrap();
            support::scan(&library.hito, &library.actor, &library.shared).await;
            let other = library
                .hito
                .catalog
                .recordings(&library.actor, Default::default())
                .await
                .unwrap()
                .items
                .into_iter()
                .find(|recording| recording.title == "Other performer track")
                .unwrap()
                .id;
            let foreign = choose(library, &other, catalog::BETA).await;
            let credit = imported_credit(library).await;
            let rejected = request(&foreign);
            assert!(matches!(
                apply(
                    library,
                    &library.actor,
                    &credit,
                    ApplyArtistIdentityToCredit {
                        request_id: rejected.request_id,
                        group_id: foreign.clone(),
                        after: None,
                        limit: None,
                    }
                )
                .await,
                Err(Error::Conflict(_))
            ));
            for recording in [&library.imported, &library.namesake] {
                let current = choice(library, &library.actor, recording).await;
                assert_eq!((group_of(&current), current.revision), (None, 0));
            }
            let alpha = choose(library, &library.local, catalog::ALPHA).await;
            let accepted = apply(
                library,
                &library.actor,
                &credit,
                ApplyArtistIdentityToCredit {
                    request_id: rejected.request_id,
                    group_id: alpha,
                    after: None,
                    limit: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(
                accepted.applied, 2,
                "A rejected request leaves no receipt behind"
            );
        })
    })
    .await;
}

#[tokio::test]
async fn retired_choices_for_another_artist_are_kept_and_retired_choices_for_this_artist_are_renewed()
 {
    run(|library| {
        Box::pin(async move {
            let alpha = choose(library, &library.local, catalog::ALPHA).await;
            assert_eq!(
                choose(library, &library.imported, catalog::ALPHA).await,
                alpha
            );
            let beta = choose(library, &library.namesake, catalog::BETA).await;
            retire(library, &[&library.imported, &library.namesake]).await;
            let retired = choice(library, &library.actor, &library.namesake).await;
            assert!(matches!(
                retired.state,
                RecordingArtistIdentityState::NeedsReview(_)
            ));
            let credit = imported_credit(library).await;
            let receipt = apply(library, &library.actor, &credit, request(&alpha))
                .await
                .unwrap();
            assert_eq!(
                (
                    receipt.applied,
                    receipt.unchanged,
                    receipt.skipped_different,
                    receipt.skipped_unavailable
                ),
                (1, 0, 1, 0)
            );
            let kept = choice(library, &library.actor, &library.namesake).await;
            assert_eq!(kept.revision, retired.revision);
            match kept.state {
                RecordingArtistIdentityState::NeedsReview(selection) => {
                    assert_eq!(selection.group_id, beta)
                }
                _ => panic!("The retired choice for another artist must stay as it was"),
            }
            assert_eq!(
                group_of(&choice(library, &library.actor, &library.imported).await),
                Some(alpha.as_str())
            );
        })
    })
    .await;
}

#[tokio::test]
async fn a_group_with_only_stale_members_cannot_be_applied_by_identifier() {
    run(|library| Box::pin(async move {
        let alpha = choose(library, &library.local, catalog::ALPHA).await;
        let pool = sqlx::SqlitePool::connect_with(
            SqliteConnectOptions::new().filename(library.database()),
        )
        .await
        .unwrap();
        sqlx::query("UPDATE recording_artist_identity_choices SET artist_operation_id='stale' WHERE recording_id=?")
            .bind(&library.local)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let credit = imported_credit(library).await;
        assert!(matches!(
            apply(library, &library.actor, &credit, request(&alpha)).await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(
            group_of(&choice(library, &library.actor, &library.imported).await),
            None
        );
    }))
    .await;
}

use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[allow(dead_code)]
#[path = "fixtures/artist_identity_selection_library.rs"]
mod library;
#[allow(unused_imports)]
use hito_core::test_support::support;

use futures_util::{FutureExt, future::BoxFuture};
use hito_core::{
    Actor, ApplyArtistIdentityToCredit, ArtistAssociation, ArtistCreditRole,
    AssociateRecordingArtist, Browse, ChooseRecordingArtistIdentity, Credentials, Error,
    MetadataProvider, PromoteArtistCorrection, PromotionSource, RecordingArtistIdentityState,
    SplitArtistIdentityGroup, SplitDestination, UndoRecordingArtistChange,
    UndoSharedArtistCorrection, UnlinkRecordingArtist, UpdateMetadataSettings,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

const M1: &str = "11111111-1111-4111-8111-111111111111";
const M2: &str = "22222222-2222-4222-8222-222222222222";

async fn run(exercise: for<'a> fn(&'a library::Library) -> BoxFuture<'a, ()>) {
    let mut library = library::Library::prepare().await;
    let settings = library
        .hito
        .catalog
        .metadata_settings(&library.actor)
        .await
        .unwrap();
    library
        .hito
        .catalog
        .set_metadata_settings(
            &library.actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: vec![
                    library.metadata_provider.clone(),
                    MetadataProvider::BuiltinMusicBrainz,
                ],
            },
        )
        .await
        .unwrap();
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.origin.shutdown().await;
    library.catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn account(library: &library::Library, name: &str) -> Actor {
    let user = library
        .hito
        .identity
        .create_user(
            &library.actor,
            Credentials {
                username: name.into(),
                password: "owned correction peer password".into(),
            },
        )
        .await
        .unwrap();
    library
        .hito
        .identity
        .set_library_access(&library.actor, &library.shared, &user.id, true)
        .await
        .unwrap();
    let session = library
        .hito
        .identity
        .login(Credentials {
            username: name.into(),
            password: "owned correction peer password".into(),
        })
        .await
        .unwrap();
    library
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap()
}

async fn link(
    library: &library::Library,
    actor: &Actor,
    recording: &str,
    artist: &str,
) -> ArtistAssociation {
    let catalog = &library.hito.catalog;
    let context = catalog
        .begin_artist_review(actor, recording, MetadataProvider::BuiltinMusicBrainz)
        .await
        .unwrap();
    catalog
        .associate_recording_artist(
            actor,
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
        .unwrap()
}

async fn choose(
    library: &library::Library,
    actor: &Actor,
    recording: &str,
    artist: &str,
) -> String {
    let association = link(library, actor, recording, artist).await;
    let catalog = &library.hito.catalog;
    let context = catalog
        .begin_artist_review(actor, recording, MetadataProvider::BuiltinMusicBrainz)
        .await
        .unwrap();
    let revision = catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap()
        .revision;
    catalog
        .choose_recording_artist_identity(
            actor,
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

async fn listed(library: &library::Library, actor: &Actor, recording: &str) -> usize {
    library
        .hito
        .catalog
        .recording_artist_associations(actor, recording)
        .await
        .unwrap()
        .len()
}

async fn chosen(library: &library::Library, actor: &Actor, recording: &str) -> bool {
    matches!(
        library
            .hito
            .catalog
            .recording_artist_identity_choice(actor, recording)
            .await
            .unwrap()
            .state,
        RecordingArtistIdentityState::Selected(_)
    )
}

async fn groups(library: &library::Library, actor: &Actor) -> i64 {
    library
        .hito
        .catalog
        .artist_identity_groups(actor, Default::default())
        .await
        .unwrap()
        .total
}

async fn unlink(library: &library::Library, recording: &str, association: &str) {
    let revision = library
        .hito
        .catalog
        .recording_artist_association_changes(&library.actor, recording)
        .await
        .unwrap()
        .revision;
    library
        .hito
        .catalog
        .unlink_recording_artist(
            &library.actor,
            recording,
            UnlinkRecordingArtist {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
                association_id: Uuid::parse_str(association).unwrap(),
            },
        )
        .await
        .unwrap();
}

fn promote(revision: i64, source: PromotionSource) -> PromoteArtistCorrection {
    PromoteArtistCorrection {
        request_id: Uuid::new_v4(),
        expected_revision: revision,
        source,
    }
}

fn unlinked(recording: &str, association: &str) -> PromotionSource {
    PromotionSource::Unlink {
        recording_id: recording.into(),
        association_id: Uuid::parse_str(association).unwrap(),
    }
}

#[tokio::test]
async fn an_administrator_can_share_an_unlink_while_personal_reviews_still_win() {
    run(|library| {
        Box::pin(async move {
            let (actor, local) = (&library.actor, &library.local);
            let peer = &library.peer;
            let second = account(library, "second-peer").await;
            let mine = link(library, actor, local, M1).await;
            link(library, peer, local, M1).await;
            link(library, &second, local, M1).await;
            let before = library.snapshot().await;
            assert!(matches!(
                library.hito.catalog.shared_artist_corrections(peer).await,
                Err(Error::Forbidden)
            ));
            unlink(library, local, &mine.id).await;
            let offered = library
                .hito
                .catalog
                .recording_artist_association_changes(actor, local)
                .await
                .unwrap();
            assert!(
                offered.shareable && offered.shared.is_none(),
                "The administrator can share a built-in unlink"
            );
            assert!(
                !library
                    .hito
                    .catalog
                    .recording_artist_association_changes(peer, local)
                    .await
                    .unwrap()
                    .shareable,
                "Other accounts are not offered the action"
            );
            assert!(matches!(
                library
                    .hito
                    .catalog
                    .promote_artist_correction(peer, promote(0, unlinked(local, &mine.id)))
                    .await,
                Err(Error::Forbidden)
            ));
            assert_eq!(
                listed(library, peer, local).await,
                1,
                "Nothing is shared yet"
            );

            let request = promote(0, unlinked(local, &mine.id));
            let receipt = library
                .hito
                .catalog
                .promote_artist_correction(actor, request.clone())
                .await
                .unwrap();
            assert!((receipt.revision, receipt.applied, receipt.recordings) == (1, true, 1));
            assert_eq!(
                library
                    .hito
                    .catalog
                    .promote_artist_correction(actor, request)
                    .await
                    .unwrap(),
                receipt,
                "An exact replay returns the stored receipt"
            );
            assert!(matches!(
                library
                    .hito
                    .catalog
                    .promote_artist_correction(
                        actor,
                        PromoteArtistCorrection {
                            request_id: receipt.request_id,
                            ..promote(5, unlinked(local, &mine.id))
                        }
                    )
                    .await,
                Err(Error::Conflict(_))
            ));
            assert!(
                matches!(
                    library
                        .hito
                        .catalog
                        .promote_artist_correction(actor, promote(0, unlinked(local, &mine.id)))
                        .await,
                    Err(Error::Conflict(_))
                ),
                "A stale revision conflicts"
            );
            assert!(
                matches!(
                    library
                        .hito
                        .catalog
                        .promote_artist_correction(actor, promote(1, unlinked(local, &mine.id)))
                        .await,
                    Err(Error::Conflict(_))
                ),
                "The same correction is not shared twice"
            );

            assert_eq!(listed(library, peer, local).await, 0);
            assert_eq!(listed(library, &second, local).await, 0);
            let offered = library
                .hito
                .catalog
                .recording_artist_association_changes(actor, local)
                .await
                .unwrap();
            assert!(!offered.shareable);
            assert_eq!(
                offered.shared.map(|shared| (shared.id, shared.revision)),
                Some((receipt.correction_id.clone(), 1))
            );
            let list = library
                .hito
                .catalog
                .shared_artist_corrections(actor)
                .await
                .unwrap();
            assert_eq!(list.revision, 1);
            assert!(
                list.items[0].applied
                    && list.items[0].promoted_by_me
                    && list.items[0].recordings == 1
            );

            link(library, &second, local, M1).await;
            assert_eq!(
                listed(library, &second, local).await,
                1,
                "A review made after sharing is personal and wins"
            );
            assert_eq!(listed(library, peer, local).await, 0);

            let undone = library
                .hito
                .catalog
                .undo_shared_artist_correction(
                    actor,
                    &receipt.correction_id,
                    UndoSharedArtistCorrection {
                        request_id: Uuid::new_v4(),
                        expected_revision: 1,
                    },
                )
                .await
                .unwrap();
            assert!(!undone.applied && undone.revision == 2);
            assert_eq!(
                listed(library, peer, local).await,
                1,
                "Withdrawing restores the shared link"
            );
            assert_eq!(listed(library, &second, local).await, 1);
            assert_eq!(
                listed(library, actor, local).await,
                0,
                "The administrator's own unlink stays"
            );
            assert!(matches!(
                library
                    .hito
                    .catalog
                    .undo_shared_artist_correction(
                        peer,
                        &receipt.correction_id,
                        UndoSharedArtistCorrection {
                            request_id: Uuid::new_v4(),
                            expected_revision: 2
                        },
                    )
                    .await,
                Err(Error::Forbidden)
            ));
            assert!(matches!(
                library
                    .hito
                    .catalog
                    .undo_shared_artist_correction(
                        actor,
                        &receipt.correction_id,
                        UndoSharedArtistCorrection {
                            request_id: Uuid::new_v4(),
                            expected_revision: 1
                        },
                    )
                    .await,
                Err(Error::Conflict(_))
            ));
            let again = library
                .hito
                .catalog
                .undo_shared_artist_correction(
                    actor,
                    &receipt.correction_id,
                    UndoSharedArtistCorrection {
                        request_id: Uuid::new_v4(),
                        expected_revision: 2,
                    },
                )
                .await
                .unwrap();
            assert!(
                again.applied && again.revision == 3,
                "Undoing the undo shares it again"
            );
            assert_eq!(listed(library, peer, local).await, 0);
            assert_eq!(
                listed(library, &second, local).await,
                1,
                "Personal reviews still win after a restore"
            );
            assert_eq!(
                library.snapshot().await,
                before,
                "Recordings, sources and personal state are unchanged"
            );
            library
                .hito
                .catalog
                .undo_recording_artist_change(
                    actor,
                    local,
                    UndoRecordingArtistChange {
                        request_id: Uuid::new_v4(),
                        expected_revision: 2,
                    },
                )
                .await
                .unwrap();
            assert_eq!(
                listed(library, actor, local).await,
                1,
                "Undoing the administrator's own unlink is a personal override and wins"
            );

            let http = {
                let context = library
                    .hito
                    .catalog
                    .begin_artist_review(
                        actor,
                        &library.imported,
                        library.metadata_provider.clone(),
                    )
                    .await
                    .unwrap();
                library
                    .hito
                    .catalog
                    .confirm_artist_review(actor, &context, catalog::ALPHA)
                    .await
                    .unwrap()
            };
            unlink(library, &library.imported, &http.id).await;
            assert!(
                matches!(
                    library
                        .hito
                        .catalog
                        .promote_artist_correction(
                            actor,
                            promote(1, unlinked(&library.imported, &http.id))
                        )
                        .await,
                    Err(Error::Invalid(_))
                ),
                "Only built-in provider corrections are shareable"
            );
        })
    })
    .await;
}

#[tokio::test]
async fn an_administrator_can_share_a_split_and_withdraw_it() {
    run(|library| {
        Box::pin(async move {
            let (actor, local) = (&library.actor, &library.local);
            let peer = &library.peer;
            let first = choose(library, actor, local, M1).await;
            let credit = library
                .hito
                .catalog
                .artists(
                    actor,
                    Browse {
                        q: Some(catalog::CREDIT.into()),
                        ..Default::default()
                    },
                )
                .await
                .unwrap()
                .items
                .into_iter()
                .find(|artist| artist.library_id != library.shared)
                .unwrap()
                .id;
            library
                .hito
                .catalog
                .apply_artist_identity_to_credit(
                    actor,
                    &credit,
                    ApplyArtistIdentityToCredit {
                        request_id: Uuid::new_v4(),
                        group_id: first.clone(),
                        after: None,
                        limit: None,
                    },
                )
                .await
                .unwrap();
            choose(library, peer, local, M1).await;
            assert_eq!(groups(library, peer).await, 1);
            link(library, actor, local, M2).await;

            let moved = library
                .hito
                .catalog
                .split_artist_identity_group(
                    actor,
                    &first,
                    SplitArtistIdentityGroup {
                        request_id: Uuid::new_v4(),
                        expected_revision: 0,
                        recordings: vec![Uuid::parse_str(local).unwrap()],
                        releases: vec![],
                        destination: SplitDestination::Identity {
                            provider: MetadataProvider::BuiltinMusicBrainz,
                            provider_artist_id: M2.into(),
                        },
                    },
                )
                .await
                .unwrap();
            let state = library
                .hito
                .catalog
                .artist_identity_group_changes(actor, &first)
                .await
                .unwrap();
            assert!(state.latest.unwrap().shareable);
            assert!(
                chosen(library, peer, local).await,
                "The split is personal until shared"
            );

            let receipt = library
                .hito
                .catalog
                .promote_artist_correction(
                    actor,
                    promote(
                        0,
                        PromotionSource::Split {
                            group_id: first.clone(),
                            revision: moved.revision,
                        },
                    ),
                )
                .await
                .unwrap();
            assert_eq!((receipt.applied, receipt.recordings), (true, 1));
            assert!(!chosen(library, peer, local).await);
            assert_eq!(
                groups(library, peer).await,
                0,
                "The recording leaves the artist for every account"
            );
            assert!(
                chosen(library, actor, local).await,
                "The administrator's destination stays"
            );
            let latest = library
                .hito
                .catalog
                .artist_identity_group_changes(actor, &first)
                .await
                .unwrap()
                .latest
                .unwrap();
            let shared = latest.shared.clone().unwrap();
            assert_eq!(
                (shared.id.as_str(), shared.revision),
                (receipt.correction_id.as_str(), 1)
            );
            assert!(!latest.shareable);
            assert!(matches!(
                library
                    .hito
                    .catalog
                    .promote_artist_correction(
                        actor,
                        promote(
                            1,
                            PromotionSource::Split {
                                group_id: first.clone(),
                                revision: moved.revision
                            }
                        )
                    )
                    .await,
                Err(Error::Conflict(_))
            ));
            assert!(matches!(
                library
                    .hito
                    .catalog
                    .promote_artist_correction(
                        actor,
                        promote(
                            1,
                            PromotionSource::Split {
                                group_id: first.clone(),
                                revision: 7
                            }
                        )
                    )
                    .await,
                Err(Error::Conflict(_))
            ));

            library
                .hito
                .catalog
                .undo_shared_artist_correction(
                    actor,
                    &receipt.correction_id,
                    UndoSharedArtistCorrection {
                        request_id: Uuid::new_v4(),
                        expected_revision: 1,
                    },
                )
                .await
                .unwrap();
            assert!(
                chosen(library, peer, local).await,
                "Withdrawing returns the other account's choice"
            );
            assert_eq!(groups(library, peer).await, 1);
        })
    })
    .await;
}

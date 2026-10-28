use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[allow(dead_code)]
#[path = "fixtures/artist_identity_selection_library.rs"]
mod library;
#[allow(unused_imports)]
use hito_core::test_support::support;

use futures_util::{FutureExt, future::BoxFuture};
use hito_core::{
    Actor, ArtistAssociation, ArtistCreditRole, AssociateRecordingArtist, Browse,
    ChooseRecordingArtistIdentity, Error, MetadataProvider, PromoteArtistCorrection,
    PromotionSource, SharedArtistCorrectionReceipt, SplitArtistIdentityGroup, SplitDestination,
    UndoArtistIdentityChange, UndoSharedArtistCorrection, UnlinkRecordingArtist,
    UpdateMetadataSettings,
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

const M1: &str = "11111111-1111-4111-8111-111111111111";
const M2: &str = "22222222-2222-4222-8222-222222222222";
const M3: &str = "33333333-3333-4333-8333-333333333333";
const RETAGGED: &str = "Retagged Performer";

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

async fn link(library: &library::Library, recording: &str, artist: &str) -> ArtistAssociation {
    let catalog = &library.hito.catalog;
    let context = catalog
        .begin_artist_review(
            &library.actor,
            recording,
            MetadataProvider::BuiltinMusicBrainz,
        )
        .await
        .unwrap();
    catalog
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
        .unwrap()
}

async fn choose(library: &library::Library, recording: &str, artist: &str) -> String {
    let association = link(library, recording, artist).await;
    let catalog = &library.hito.catalog;
    let context = catalog
        .begin_artist_review(
            &library.actor,
            recording,
            MetadataProvider::BuiltinMusicBrainz,
        )
        .await
        .unwrap();
    let revision = catalog
        .recording_artist_identity_choice(&library.actor, recording)
        .await
        .unwrap()
        .revision;
    catalog
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

async fn share(library: &library::Library, source: PromotionSource) {
    let revision = library
        .hito
        .catalog
        .shared_artist_corrections(&library.actor)
        .await
        .unwrap()
        .revision;
    library
        .hito
        .catalog
        .promote_artist_correction(
            &library.actor,
            PromoteArtistCorrection {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
                source,
            },
        )
        .await
        .unwrap();
}

const TAGGED: &str = "Tagged Performer";

fn tag_file(library: &library::Library, artist: &str) {
    let path = library.music().join("tagged.wav");
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Tagged recording".into());
    tag.set_artist(artist.into());
    tag.set_album("Tagged album".into());
    tag.insert_text(ItemKey::AlbumArtist, "Tagged album credit".into());
    tag.save_to_path(&path, Default::default()).unwrap();
}

async fn tagged(library: &library::Library) -> String {
    support::write_wav(&library.music().join("tagged.wav"), 3);
    tag_file(library, TAGGED);
    support::scan(&library.hito, &library.actor, &library.shared).await;
    let recordings = library
        .hito
        .catalog
        .recordings(&library.actor, Default::default())
        .await
        .unwrap()
        .items;
    let mut found = Vec::new();
    for recording in recordings {
        if credit_of(library, &library.actor, &recording.id).await == TAGGED {
            found.push(recording.id);
        }
    }
    assert_eq!(found.len(), 1);
    found.remove(0)
}

async fn retag(library: &library::Library, artist: &str) {
    tag_file(library, artist);
    support::scan(&library.hito, &library.actor, &library.shared).await;
}

async fn credit_of(library: &library::Library, actor: &Actor, recording: &str) -> String {
    library
        .hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap()
        .artist
        .value
}

#[tokio::test]
async fn a_raw_retag_retires_stale_corrections_visibly_and_keeps_the_valid_ones() {
    run(|library| {
        Box::pin(async move {
            let (actor, imported) = (&library.actor, &library.imported);
            let local = &tagged(library).await;
            let first = choose(library, local, M1).await;
            link(library, local, M2).await;
            let stale_link = link(library, local, M3).await;
            unlink(library, local, &stale_link.id).await;
            let kept = link(library, imported, M1).await;
            unlink(library, imported, &kept.id).await;
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
            share(
                library,
                PromotionSource::Unlink {
                    recording_id: local.clone(),
                    association_id: Uuid::parse_str(&stale_link.id).unwrap(),
                },
            )
            .await;
            share(
                library,
                PromotionSource::Split {
                    group_id: first.clone(),
                    revision: moved.revision,
                },
            )
            .await;
            share(
                library,
                PromotionSource::Unlink {
                    recording_id: imported.clone(),
                    association_id: Uuid::parse_str(&kept.id).unwrap(),
                },
            )
            .await;
            let corrections = library
                .hito
                .catalog
                .shared_artist_corrections(actor)
                .await
                .unwrap();
            assert!(
                corrections
                    .items
                    .iter()
                    .all(|item| item.retired == 0 && item.applied)
            );
            let original = credit_of(library, actor, local).await;
            let unrelated = credit_of(library, actor, imported).await;

            retag(library, RETAGGED).await;
            assert_eq!(
                credit_of(library, actor, local).await,
                RETAGGED,
                "The rescan must pick up the raw retag"
            );
            assert_eq!(credit_of(library, actor, imported).await, unrelated);

            let changes = library
                .hito
                .catalog
                .artist_identity_group_changes(actor, &first)
                .await
                .unwrap();
            let latest = changes.latest.unwrap();
            assert_eq!(
                (changes.revision, latest.recordings, latest.retired),
                (1, 0, 1)
            );
            assert!(!latest.undoable && !latest.shareable);
            let own = library
                .hito
                .catalog
                .recording_artist_association_changes(actor, local)
                .await
                .unwrap();
            assert_eq!(own.retired.len(), 1);
            assert_eq!(own.retired[0].association_id, stale_link.id);
            let valid = library
                .hito
                .catalog
                .recording_artist_association_changes(actor, imported)
                .await
                .unwrap();
            assert!(
                valid.retired.is_empty(),
                "A correction whose credit is unchanged stays active"
            );
            assert_eq!(
                library
                    .hito
                    .catalog
                    .recording_artist_associations(actor, imported)
                    .await
                    .unwrap()
                    .len(),
                0,
                "The unchanged unlink still hides its association"
            );
            let corrections = library
                .hito
                .catalog
                .shared_artist_corrections(actor)
                .await
                .unwrap();
            let mut retired = corrections
                .items
                .iter()
                .map(|item| (item.recordings, item.retired))
                .collect::<Vec<_>>();
            retired.sort_unstable();
            assert_eq!(retired, vec![(1, 0), (1, 1), (1, 1)]);
            assert!(corrections.items.iter().all(|item| item.applied));
            assert!(
                matches!(
                    library
                        .hito
                        .catalog
                        .undo_artist_identity_change(
                            actor,
                            &first,
                            UndoArtistIdentityChange {
                                request_id: Uuid::new_v4(),
                                expected_revision: 1
                            }
                        )
                        .await,
                    Err(Error::Conflict(_))
                ),
                "A change that no longer applies cannot be undone"
            );
            let credits = library
                .hito
                .catalog
                .artists(
                    actor,
                    Browse {
                        q: Some(RETAGGED.into()),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            assert_eq!(
                credits.items.len(),
                1,
                "The retag creates one credit and no duplicate identity"
            );

            retag(library, TAGGED).await;
            assert_eq!(credit_of(library, actor, local).await, original);
            let again = library
                .hito
                .catalog
                .artist_identity_group_changes(actor, &first)
                .await
                .unwrap();
            assert_eq!(
                again.latest.as_ref().map(|l| (l.recordings, l.retired)),
                Some((0, 1))
            );
            let again = library
                .hito
                .catalog
                .recording_artist_association_changes(actor, local)
                .await
                .unwrap();
            assert_eq!(
                again.retired.len(),
                1,
                "Returning to the old tags does not revive retired corrections"
            );
            assert_eq!(
                library
                    .hito
                    .catalog
                    .artists(
                        actor,
                        Browse {
                            q: Some(RETAGGED.into()),
                            ..Default::default()
                        }
                    )
                    .await
                    .unwrap()
                    .items
                    .len(),
                0,
                "The unused retagged credit no longer lists a recording"
            );
        })
    })
    .await;
}

async fn peer_account(library: &library::Library) -> Actor {
    let user = library
        .hito
        .identity
        .create_user(
            &library.actor,
            hito_core::Credentials {
                username: "reconcile-peer".into(),
                password: "owned reconcile peer password".into(),
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
        .login(hito_core::Credentials {
            username: "reconcile-peer".into(),
            password: "owned reconcile peer password".into(),
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

async fn peer_link(library: &library::Library, peer: &Actor, recording: &str) {
    let catalog = &library.hito.catalog;
    let context = catalog
        .begin_artist_review(peer, recording, MetadataProvider::BuiltinMusicBrainz)
        .await
        .unwrap();
    catalog
        .associate_recording_artist(
            peer,
            recording,
            AssociateRecordingArtist {
                artist_id: context.artist_id.clone(),
                source_id: context.source_id.clone(),
                observation_id: context.observation_id.clone(),
                role: ArtistCreditRole::RecordingArtist,
                provider: MetadataProvider::BuiltinMusicBrainz,
                provider_artist_id: M1.into(),
                policy_revision: context.policy_revision,
            },
        )
        .await
        .unwrap();
}

async fn visible(library: &library::Library, actor: &Actor, recording: &str) -> usize {
    library
        .hito
        .catalog
        .recording_artist_associations(actor, recording)
        .await
        .unwrap()
        .len()
}

async fn share_unlink(
    library: &library::Library,
    recording: &str,
    association: &str,
) -> Result<SharedArtistCorrectionReceipt, Error> {
    let revision = library
        .hito
        .catalog
        .shared_artist_corrections(&library.actor)
        .await
        .unwrap()
        .revision;
    library
        .hito
        .catalog
        .promote_artist_correction(
            &library.actor,
            PromoteArtistCorrection {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
                source: PromotionSource::Unlink {
                    recording_id: recording.into(),
                    association_id: Uuid::parse_str(association).unwrap(),
                },
            },
        )
        .await
}

async fn toggle(
    library: &library::Library,
    correction: &str,
    revision: i64,
) -> Result<SharedArtistCorrectionReceipt, Error> {
    library
        .hito
        .catalog
        .undo_shared_artist_correction(
            &library.actor,
            correction,
            UndoSharedArtistCorrection {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
            },
        )
        .await
}

#[tokio::test]
async fn retired_shared_subjects_stop_hiding_and_never_block_a_fresh_share_or_a_restore() {
    run(|library| {
        Box::pin(async move {
            let actor = &library.actor;
            let local = &tagged(library).await;
            let peer = peer_account(library).await;
            peer_link(library, &peer, local).await;
            let mine = link(library, local, M1).await;
            unlink(library, local, &mine.id).await;
            let first = share_unlink(library, local, &mine.id).await.unwrap();
            assert_eq!(visible(library, &peer, local).await, 0);

            retag(library, RETAGGED).await;
            let listed = library.hito.catalog.shared_artist_corrections(actor).await.unwrap();
            assert_eq!((listed.items[0].applied, listed.items[0].retired), (true, 1));
            assert_eq!(visible(library, &peer, local).await, 1, "A retired subject no longer hides an association");
            retag(library, TAGGED).await;

            let fresh = share_unlink(library, local, &mine.id).await.unwrap();
            assert_ne!(fresh.correction_id, first.correction_id, "A retired subject does not block a fresh share");
            assert_eq!(visible(library, &peer, local).await, 0);

            let withdrawn = toggle(library, &first.correction_id, 1).await.unwrap();
            assert!(!withdrawn.applied);
            assert!(
                matches!(toggle(library, &first.correction_id, 2).await, Err(Error::Conflict(_))),
                "A fully retired correction cannot be restored"
            );
            let second = toggle(library, &fresh.correction_id, 1).await.unwrap();
            assert!(!second.applied);
            let again = share_unlink(library, local, &mine.id).await.unwrap();
            assert!(
                matches!(toggle(library, &fresh.correction_id, 2).await, Err(Error::Conflict(_))),
                "A withdrawn correction cannot be restored over an active one for the same recording"
            );
            assert!(again.applied);
        })
    })
    .await;
}

#[tokio::test]
async fn an_unchanged_artist_with_a_new_metadata_observation_retires_the_shared_subject_visibly() {
    run(|library| {
        Box::pin(async move {
            let actor = &library.actor;
            let local = &tagged(library).await;
            let mine = link(library, local, M1).await;
            unlink(library, local, &mine.id).await;
            share_unlink(library, local, &mine.id).await.unwrap();
            let path = library.music().join("tagged.wav");
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title("Tagged recording".into());
            tag.set_artist(TAGGED.into());
            tag.set_album("A different album after rewriting the file".into());
            tag.insert_text(ItemKey::AlbumArtist, "Tagged album credit".into());
            tag.save_to_path(&path, Default::default()).unwrap();
            support::scan(&library.hito, actor, &library.shared).await;
            assert_eq!(credit_of(library, actor, local).await, TAGGED);
            let listed = library.hito.catalog.shared_artist_corrections(actor).await.unwrap();
            assert_eq!(
                (listed.items[0].applied, listed.items[0].retired),
                (true, 1),
                "A subject bound to a replaced observation is retired, not shown as an applied no-op"
            );
        })
    })
    .await;
}

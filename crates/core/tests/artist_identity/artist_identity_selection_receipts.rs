use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[path = "../fixtures/artist_identity_selection_library.rs"]
mod library;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    ChooseRecordingArtistIdentity, Error, RecordingArtistIdentityState,
    ResetRecordingArtistIdentity,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn identity_receipts_reject_stale_changed_and_cross_command_reuse_in_their_account_namespace()
{
    let mut library = library::Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.origin.shutdown().await;
    library.catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &library::Library) {
    let hito = &library.hito;
    let actor = &library.actor;
    let recording = &library.local;
    let context = hito
        .artist_catalog
        .begin_artist_review(actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    let association = hito
        .artist_catalog
        .confirm_artist_review(actor, &context, catalog::ALPHA)
        .await
        .unwrap();
    let before = library.snapshot().await;
    let requests = library.catalog.requests();
    let origin = library.origin_requests().await;
    let choose_id = Uuid::new_v4();
    let chosen = hito
        .artist_catalog
        .choose_recording_artist_identity(
            actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: choose_id,
                expected_revision: 0,
                association_id: Uuid::parse_str(&association.id).unwrap(),
                context: context.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(chosen.revision, 1);
    for (request_id, expected_revision) in [(Uuid::new_v4(), 0), (choose_id, 1)] {
        assert!(
            matches!(
                hito.artist_catalog
                    .choose_recording_artist_identity(
                        actor,
                        recording,
                        ChooseRecordingArtistIdentity {
                            request_id,
                            expected_revision,
                            association_id: Uuid::parse_str(&association.id).unwrap(),
                            context: context.clone(),
                        }
                    )
                    .await,
                Err(Error::Conflict(_))
            ),
            "Stale choose or changed contents must not replace the accepted identity"
        );
    }
    for (request_id, expected_revision) in [(Uuid::new_v4(), 0), (choose_id, 1)] {
        assert!(
            matches!(
                hito.artist_catalog
                    .reset_recording_artist_identity(
                        actor,
                        recording,
                        ResetRecordingArtistIdentity {
                            request_id,
                            expected_revision,
                        }
                    )
                    .await,
                Err(Error::Conflict(_))
            ),
            "Stale reset or a choose request reused as reset must not clear the identity"
        );
    }
    let reset_id = Uuid::new_v4();
    let reset = hito
        .artist_catalog
        .reset_recording_artist_identity(
            actor,
            recording,
            ResetRecordingArtistIdentity {
                request_id: reset_id,
                expected_revision: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(reset.revision, 2);
    assert!(
        matches!(
            hito.artist_catalog
                .reset_recording_artist_identity(
                    actor,
                    recording,
                    ResetRecordingArtistIdentity {
                        request_id: reset_id,
                        expected_revision: 2,
                    }
                )
                .await,
            Err(Error::Conflict(_))
        ),
        "Changed reset contents must conflict even when their expected revision is current"
    );
    assert!(
        matches!(
            hito.artist_catalog
                .choose_recording_artist_identity(
                    actor,
                    recording,
                    ChooseRecordingArtistIdentity {
                        request_id: reset_id,
                        expected_revision: 2,
                        association_id: Uuid::parse_str(&association.id).unwrap(),
                        context,
                    }
                )
                .await,
            Err(Error::Conflict(_))
        ),
        "A reset request cannot be reused as a choose command"
    );
    assert!(
        matches!(
            hito.artist_catalog
                .reset_recording_artist_identity(
                    actor,
                    &library.imported,
                    ResetRecordingArtistIdentity {
                        request_id: reset_id,
                        expected_revision: 0,
                    }
                )
                .await,
            Err(Error::Conflict(_))
        ),
        "The receipt namespace must span recordings within the account"
    );
    let peer = hito
        .artist_catalog
        .reset_recording_artist_identity(
            &library.peer,
            recording,
            ResetRecordingArtistIdentity {
                request_id: reset_id,
                expected_revision: 0,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        peer.revision, 1,
        "Another account retains its own request namespace"
    );
    let current = hito
        .artist_catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap();
    assert!(
        current.revision == 2 && matches!(current.state, RecordingArtistIdentityState::Unselected)
    );
    assert_eq!(
        hito.artist_catalog
            .recording_artist_identity_choice(actor, &library.imported)
            .await
            .unwrap()
            .revision,
        0
    );
    assert_eq!(
        hito.artist_catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.catalog.requests(), requests);
    assert_eq!(library.origin_requests().await, origin);
}

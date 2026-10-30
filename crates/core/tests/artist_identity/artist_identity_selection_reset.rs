use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[path = "../fixtures/artist_identity_selection_library.rs"]
mod library;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    ChooseRecordingArtistIdentity, RecordingArtistIdentityState, ResetRecordingArtistIdentity,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn reset_retains_a_tombstone_and_replays_acknowledgements_without_restoring_old_choices() {
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
    let before = library.snapshot().await;
    let requests = library.catalog.requests();
    let origin = library.origin_requests().await;
    let reset_id = Uuid::new_v4();
    let initial = hito.catalog.reset_recording_artist_identity(actor, recording, ResetRecordingArtistIdentity {
        request_id: reset_id, expected_revision: 0,
    }).await.expect("Explicit reset must retain a revisioned unselected tombstone, including from revision zero");
    assert_eq!(initial.revision, 1);
    assert!(initial.selection.is_none());
    let choice = hito
        .catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap();
    assert!(
        choice.revision == 1 && matches!(choice.state, RecordingArtistIdentityState::Unselected)
    );
    let context = hito
        .catalog
        .begin_artist_review(actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    let association = hito
        .catalog
        .confirm_artist_review(actor, &context, catalog::ALPHA)
        .await
        .unwrap();
    let choose_id = Uuid::new_v4();
    let selected = hito
        .catalog
        .choose_recording_artist_identity(
            actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: choose_id,
                expected_revision: 1,
                association_id: Uuid::parse_str(&association.id).unwrap(),
                context: context.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(selected.revision, 2);
    let replay = hito
        .catalog
        .reset_recording_artist_identity(
            actor,
            recording,
            ResetRecordingArtistIdentity {
                request_id: reset_id,
                expected_revision: 0,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(initial).unwrap()
    );
    let current = hito
        .catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap();
    assert!(
        current.revision == 2 && matches!(current.state, RecordingArtistIdentityState::Selected(_)),
        "Replaying the earlier reset must not clear a newer accepted selection"
    );
    let final_id = Uuid::new_v4();
    let reset = hito
        .catalog
        .reset_recording_artist_identity(
            actor,
            recording,
            ResetRecordingArtistIdentity {
                request_id: final_id,
                expected_revision: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(reset.revision, 3);
    assert!(reset.selection.is_none());
    let replay = hito
        .catalog
        .choose_recording_artist_identity(
            actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: choose_id,
                expected_revision: 1,
                association_id: Uuid::parse_str(&association.id).unwrap(),
                context,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(selected).unwrap()
    );
    let repeated = hito
        .catalog
        .reset_recording_artist_identity(
            actor,
            recording,
            ResetRecordingArtistIdentity {
                request_id: final_id,
                expected_revision: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(repeated).unwrap(),
        serde_json::to_value(reset).unwrap()
    );
    let current = hito
        .catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap();
    assert!(
        current.revision == 3 && matches!(current.state, RecordingArtistIdentityState::Unselected)
    );
    assert_eq!(
        hito.catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        hito.catalog
            .recording_artist_associations(actor, recording)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.catalog.requests(), requests);
    assert_eq!(library.origin_requests().await, origin);
}

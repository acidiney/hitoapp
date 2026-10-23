use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[path = "fixtures/artist_identity_selection_retained_library.rs"]
mod library;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{ChooseRecordingArtistIdentity, Error, RecordingArtistIdentityState};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn a_regranted_artist_choice_survives_unchanged_metadata_but_not_an_absent_a_to_b_to_a() {
    let mut library = library::RetainedLibrary::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &library::RetainedLibrary) {
    let hito = &library.hito;
    let actor = &library.listener;
    let recording = &library.recording_id;
    let context = hito
        .catalog
        .begin_artist_review(actor, recording, library.provider.clone())
        .await
        .unwrap();
    let association = hito
        .catalog
        .confirm_artist_review(actor, &context, catalog::ALPHA)
        .await
        .unwrap();
    let receipt = hito
        .catalog
        .choose_recording_artist_identity(
            actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
                association_id: Uuid::parse_str(&association.id).unwrap(),
                context: context.clone(),
            },
        )
        .await
        .unwrap();
    let before = library.snapshot().await;
    let metadata = hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap();
    let original = serde_json::to_value(metadata.selected_observation.unwrap()).unwrap();
    assert_eq!(metadata.observations.items.len(), 1);
    let requests = library.catalog.requests();

    library.access(false).await;
    assert_eq!(
        hito.catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert!(matches!(
        hito.catalog
            .recording_artist_identity_choice(actor, recording)
            .await,
        Err(Error::NotFound)
    ));
    library.access(true).await;
    let unchanged = hito
        .catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap();
    assert_eq!(unchanged.revision, receipt.revision);
    assert!(
        matches!(&unchanged.state, RecordingArtistIdentityState::Selected(selection)
        if selection.group_id == receipt.selection.as_ref().unwrap().group_id && selection.association_id == association.id)
    );
    assert_eq!(
        hito.catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
    let fresh = hito
        .catalog
        .begin_artist_review(actor, recording, library.provider.clone())
        .await
        .unwrap();
    assert_ne!(context.access_grant_id, fresh.access_grant_id);
    let stale = hito
        .catalog
        .choose_recording_artist_identity(
            actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: unchanged.revision,
                association_id: Uuid::parse_str(&association.id).unwrap(),
                context,
            },
        )
        .await;
    assert!(
        matches!(stale, Err(Error::Conflict(_))),
        "An old grant must not authorize a new identity write after regrant"
    );
    assert_eq!(library.snapshot().await, before);

    library.access(false).await;
    library.retag().await;
    let intervening = hito
        .catalog
        .recording_metadata(&library.admin, recording, Default::default())
        .await
        .unwrap();
    let intervening = intervening.selected_observation.unwrap();
    assert_ne!(intervening.id, original["id"].as_str().unwrap());
    assert_eq!(
        intervening.source_id.as_deref(),
        original["source_id"].as_str()
    );
    assert_eq!(intervening.artist, "Intervening source performer");
    library.restore().await;
    let restored = hito
        .catalog
        .recording_metadata(&library.admin, recording, Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(restored.selected_observation.unwrap()).unwrap(),
        original,
        "Restoring the byte-exact WAV must reuse the original immutable observation before testing selection eligibility"
    );
    assert_eq!(restored.observations.items.len(), 2);
    assert!(
        restored
            .observations
            .items
            .iter()
            .any(|observation| observation.id == intervening.id)
    );
    assert_eq!(
        hito.catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    library.access(true).await;
    let changed = hito
        .catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap();
    assert!(
        matches!(&changed.state, RecordingArtistIdentityState::NeedsReview(selection)
        if selection.group_id == receipt.selection.as_ref().unwrap().group_id && selection.association_id == association.id),
        "An observation changed while access was absent must retire the choice even when the original observation returns before regrant"
    );
    assert_eq!(changed.revision, receipt.revision + 1);
    assert_eq!(
        hito.catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        library.snapshot().await,
        before,
        "Retirement must preserve exact current metadata/operations, original observation/audio, recording/release/source identities, personal state/events, association history and the other account"
    );
    assert_eq!(
        library.catalog.requests(),
        requests,
        "Grant changes, scans and local identity reads must make no catalog calls"
    );
}

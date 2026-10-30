use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[path = "../fixtures/artist_identity_selection_library.rs"]
mod library;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{Action, ChooseRecordingArtistIdentity, Error, RecordingArtistIdentityState};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn title_edits_preserve_selection_but_artist_a_to_b_to_a_requires_a_new_explicit_choice() {
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
        .catalog
        .begin_artist_review(actor, recording, library.metadata_provider.clone())
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
    let group = &receipt.selection.as_ref().unwrap().group_id;
    let metadata = hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap();
    let original_artist = serde_json::to_value(metadata.artist).unwrap();
    let original_observation = serde_json::to_value(metadata.selected_observation).unwrap();
    let sources =
        serde_json::to_value(hito.catalog.sources(actor, recording).await.unwrap()).unwrap();
    let requests = library.catalog.requests();
    let origin = library.origin_requests().await;
    let id = Uuid::parse_str(recording).unwrap();
    support::synchronize(
        hito,
        actor,
        vec![Action::RecordingTitleSet {
            recording_id: id,
            title: "Private title after artist selection".into(),
        }],
    )
    .await;
    let metadata = hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap();
    assert_eq!(
        metadata.title.personal.as_deref(),
        Some("Private title after artist selection")
    );
    assert_eq!(
        serde_json::to_value(metadata.artist).unwrap(),
        original_artist
    );
    assert_eq!(
        serde_json::to_value(metadata.selected_observation).unwrap(),
        original_observation
    );
    let choice = hito
        .catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap();
    assert!(
        choice.revision == 1 && matches!(choice.state, RecordingArtistIdentityState::Selected(_))
    );
    let detail = hito
        .catalog
        .artist_identity_group(actor, group, Default::default())
        .await
        .unwrap();
    assert_eq!(
        detail.recordings.items[0].recording.title,
        "Private title after artist selection"
    );
    support::synchronize(
        hito,
        actor,
        vec![
            Action::RecordingArtistSet {
                recording_id: id,
                artist: "Private intervening performer".into(),
            },
            Action::RecordingArtistSet {
                recording_id: id,
                artist: catalog::CREDIT.into(),
            },
        ],
    )
    .await;
    let metadata = hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap();
    assert_eq!(metadata.artist.value, catalog::CREDIT);
    assert_ne!(metadata.artist.operation_id, context.artist_operation_id);
    assert_eq!(
        serde_json::to_value(metadata.selected_observation).unwrap(),
        original_observation
    );
    assert_eq!(
        serde_json::to_value(hito.catalog.sources(actor, recording).await.unwrap()).unwrap(),
        sources
    );
    let choice = hito
        .catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap();
    assert!(
        choice.revision == 2
            && matches!(&choice.state, RecordingArtistIdentityState::NeedsReview(selection) if &selection.group_id == group)
    );
    assert_eq!(
        hito.catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    let after_edit = library.snapshot().await;
    assert!(matches!(
        hito.catalog
            .choose_recording_artist_identity(
                actor,
                recording,
                ChooseRecordingArtistIdentity {
                    request_id: Uuid::new_v4(),
                    expected_revision: 2,
                    association_id: Uuid::parse_str(&association.id).unwrap(),
                    context,
                }
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let fresh = hito
        .catalog
        .begin_artist_review(actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    let selected = hito
        .catalog
        .choose_recording_artist_identity(
            actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: 2,
                association_id: Uuid::parse_str(&association.id).unwrap(),
                context: fresh,
            },
        )
        .await
        .unwrap();
    assert_eq!(selected.revision, 3);
    assert_eq!(&selected.selection.unwrap().group_id, group);
    assert_eq!(
        hito.catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(library.snapshot().await, after_edit);
    assert_eq!(library.catalog.requests(), requests);
    assert_eq!(library.origin_requests().await, origin);
}

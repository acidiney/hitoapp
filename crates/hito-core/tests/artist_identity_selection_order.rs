#[path = "fixtures/artist_identity_selection_catalog.rs"]
mod catalog;
#[path = "fixtures/artist_identity_selection_library.rs"]
mod library;
mod support;

use futures_util::FutureExt;
use hito_core::{
    Action, ArtistIdentityGroupQuery, ChooseRecordingArtistIdentity, ResetRecordingArtistIdentity,
};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const FIRST: &str = "EarlierAcceptedCredit";
const SECOND: &str = "LaterAcceptedCredit";

#[tokio::test]
async fn group_display_and_search_follow_acceptance_order_through_reset_rechoose_and_grants() {
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
    let mut recordings = [&library.local, &library.imported];
    recordings.sort();
    let [second, first] = recordings;
    assert!(first > second);
    support::synchronize(
        hito,
        actor,
        vec![
            Action::RecordingArtistSet {
                recording_id: Uuid::parse_str(first).unwrap(),
                artist: FIRST.into(),
            },
            Action::RecordingArtistSet {
                recording_id: Uuid::parse_str(second).unwrap(),
                artist: SECOND.into(),
            },
        ],
    )
    .await;
    let first_request = review(library, first).await;
    let first_replay = serde_json::to_value(&first_request).unwrap();
    let second_request = review(library, second).await;
    let before_second = seconds();
    while seconds() == before_second {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let accepted_second = seconds();
    let first_receipt = hito
        .catalog
        .choose_recording_artist_identity(actor, first, first_request)
        .await
        .unwrap();
    let second_receipt = hito
        .catalog
        .choose_recording_artist_identity(actor, second, second_request)
        .await
        .unwrap();
    assert_eq!(
        seconds(),
        accepted_second,
        "Owned choices must complete in the same second before testing chronology"
    );
    let first_selection = first_receipt.selection.as_ref().unwrap();
    let second_selection = second_receipt.selection.as_ref().unwrap();
    let group = &first_selection.group_id;
    assert_eq!(&second_selection.group_id, group);
    let before = library.snapshot().await;
    let requests = library.catalog.requests();
    let origin = library.origin_requests().await;
    assert_label(library, group, first, FIRST, SECOND).await;
    reset(library, first, 1).await;
    assert_label(library, group, second, SECOND, FIRST).await;
    let replayed = hito
        .catalog
        .choose_recording_artist_identity(
            actor,
            first,
            serde_json::from_value(first_replay).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replayed).unwrap(),
        serde_json::to_value(&first_receipt).unwrap()
    );
    assert_label(library, group, second, SECOND, FIRST).await;
    rechoose(library, first, 2, &first_selection.association_id).await;
    assert_label(library, group, second, SECOND, FIRST).await;
    reset(library, second, 1).await;
    assert_label(library, group, first, FIRST, SECOND).await;
    rechoose(library, second, 2, &second_selection.association_id).await;
    assert_label(library, group, first, FIRST, SECOND).await;
    let imported_association = if &library.imported == first {
        &first_selection.association_id
    } else {
        &second_selection.association_id
    };
    reset(library, &library.imported, 3).await;
    rechoose(library, &library.imported, 4, imported_association).await;
    let (local_name, imported_name) = if &library.local == first {
        (FIRST, SECOND)
    } else {
        (SECOND, FIRST)
    };
    assert_label(library, group, &library.local, local_name, imported_name).await;
    hito.identity
        .set_library_access(actor, &library.shared, &actor.user().id, false)
        .await
        .unwrap();
    assert_label(library, group, &library.imported, imported_name, local_name).await;
    hito.identity
        .set_library_access(actor, &library.shared, &actor.user().id, true)
        .await
        .unwrap();
    assert_label(library, group, &library.local, local_name, imported_name).await;
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.catalog.requests(), requests);
    assert_eq!(library.origin_requests().await, origin);
}

async fn review(library: &library::Library, recording: &str) -> ChooseRecordingArtistIdentity {
    let context = library
        .hito
        .catalog
        .begin_artist_review(&library.actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    let association = library
        .hito
        .catalog
        .confirm_artist_review(&library.actor, &context, catalog::ALPHA)
        .await
        .unwrap();
    ChooseRecordingArtistIdentity {
        request_id: Uuid::new_v4(),
        expected_revision: 0,
        association_id: Uuid::parse_str(&association.id).unwrap(),
        context,
    }
}

async fn reset(library: &library::Library, recording: &str, revision: i64) {
    let receipt = library
        .hito
        .catalog
        .reset_recording_artist_identity(
            &library.actor,
            recording,
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.revision, revision + 1);
    assert!(receipt.selection.is_none());
}

async fn rechoose(library: &library::Library, recording: &str, revision: i64, association: &str) {
    let context = library
        .hito
        .catalog
        .begin_artist_review(&library.actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    let receipt = library
        .hito
        .catalog
        .choose_recording_artist_identity(
            &library.actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
                association_id: Uuid::parse_str(association).unwrap(),
                context,
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.revision, revision + 1);
}

async fn assert_label(
    library: &library::Library,
    group: &str,
    recording: &str,
    name: &str,
    hidden: &str,
) {
    let visible = library
        .hito
        .catalog
        .artist_identity_group(&library.actor, group, Default::default())
        .await
        .unwrap();
    assert_eq!(
        visible.group.name.value, name,
        "Group labels must follow the earliest accepted currently eligible credit"
    );
    assert_eq!(visible.group.name.recording_id, recording);
    for (term, count) in [(name, 1), (hidden, 0)] {
        let found = library
            .hito
            .catalog
            .artist_identity_groups(
                &library.actor,
                ArtistIdentityGroupQuery {
                    q: Some(term.into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(found.total, count);
        assert_eq!(found.items.len() as i64, count);
        if count == 1 {
            assert_eq!(found.items[0].id, group);
            assert_eq!(found.items[0].name.value, name);
            assert_eq!(found.items[0].name.recording_id, recording);
        }
    }
}

fn seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

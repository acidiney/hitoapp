#[path = "fixtures/artist_identity_selection_catalog.rs"]
mod catalog;
#[path = "fixtures/artist_identity_selection_library.rs"]
mod library;
mod support;

use futures_util::FutureExt;
use hito_core::ChooseRecordingArtistIdentity;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Barrier, task::JoinSet};
use uuid::Uuid;

#[tokio::test]
async fn concurrent_identity_writers_finish_without_exhausting_the_connection_pool() {
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
    let context = hito
        .catalog
        .begin_artist_review(actor, &library.local, library.metadata_provider.clone())
        .await
        .unwrap();
    let association = hito
        .catalog
        .confirm_artist_review(actor, &context, catalog::ALPHA)
        .await
        .unwrap();
    let request = serde_json::to_value(ChooseRecordingArtistIdentity {
        request_id: Uuid::new_v4(),
        expected_revision: 0,
        association_id: Uuid::parse_str(&association.id).unwrap(),
        context,
    })
    .unwrap();
    let before = library.snapshot().await;
    let catalog_before = library.catalog.requests();
    let origin_before = library.origin_requests().await;
    let start = Arc::new(Barrier::new(6));
    let mut writers = JoinSet::new();
    for _ in 0..5 {
        let catalog = hito.catalog.clone();
        let actor = actor.clone();
        let recording = library.local.clone();
        let request = serde_json::from_value(request.clone()).unwrap();
        let start = start.clone();
        writers.spawn(async move {
            start.wait().await;
            catalog
                .choose_recording_artist_identity(&actor, &recording, request)
                .await
        });
    }
    start.wait().await;
    let completed = tokio::time::timeout(Duration::from_secs(3), async {
        let mut receipts = Vec::new();
        while let Some(result) = writers.join_next().await {
            receipts.push(serde_json::to_value(result.unwrap().unwrap()).unwrap());
        }
        receipts
    })
    .await;
    writers.abort_all();
    while writers.join_next().await.is_some() {}
    assert!(
        completed.is_ok(),
        "Five simultaneous public identity writes must complete before the SQLite lock timeout without waiting for an extra pool connection"
    );
    let receipts = completed.unwrap();
    assert_eq!(receipts.len(), 5);
    assert!(receipts.iter().all(|receipt| receipt == &receipts[0]));
    assert_eq!(receipts[0]["revision"], 1);
    let groups = hito
        .catalog
        .artist_identity_groups(actor, Default::default())
        .await
        .unwrap();
    assert_eq!(groups.total, 1);
    assert_eq!(groups.items[0].recording_count, 1);
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.catalog.requests(), catalog_before);
    assert_eq!(library.origin_requests().await, origin_before);
}

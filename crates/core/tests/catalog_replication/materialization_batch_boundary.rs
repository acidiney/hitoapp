use super::materialization_batch_boundary_fixture as fixture;
use crate::{CatalogSnapshotMode, Error, test_support::hang_guard::hang_guard};

#[tokio::test]
async fn public_snapshot_preserves_associations_across_batch64_and_page_boundaries() {
    let fixture = hang_guard(
        "the batch-boundary fixture must be prepared",
        Box::pin(fixture::Fixture::seed()),
    )
    .await;
    let services = &fixture.services;
    let actor = &fixture.actor;
    let library = &fixture.library;
    let foreign_actor = &fixture.foreign_actor;
    let foreign_library = &fixture.foreign_library;
    let expected = &fixture.expected;

    assert!(matches!(
        services
            .catalog_replication
            .begin(foreign_actor, library)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        services
            .catalog_replication
            .begin(actor, foreign_library)
            .await,
        Err(Error::NotFound)
    ));
    let foreign_snapshot = services
        .catalog_replication
        .begin(foreign_actor, foreign_library)
        .await
        .unwrap();
    assert_eq!(foreign_snapshot.recording_count, 1);
    let foreign_page = services
        .catalog_replication
        .page(foreign_actor, foreign_snapshot.id, 0)
        .await
        .unwrap();
    assert_eq!(
        foreign_page.recordings[0].title,
        "PRIVATE-CROSS-LIBRARY-SENTINEL"
    );

    let snapshot = services
        .catalog_replication
        .begin(actor, library)
        .await
        .unwrap();
    assert_eq!(snapshot.mode, CatalogSnapshotMode::SnapshotOnly);
    assert_eq!(snapshot.account_id, actor.id());
    assert_eq!(snapshot.library_id.as_str(), library);
    assert_eq!(snapshot.recording_count, 513);
    assert_eq!(snapshot.page_count, 3);

    let mut pages = Vec::new();
    let mut serialized = Vec::new();
    for ordinal in 0..snapshot.page_count {
        let page = services
            .catalog_replication
            .page(actor, snapshot.id, ordinal)
            .await
            .unwrap();
        serialized.extend(serde_json::to_vec(&page).unwrap());
        pages.push(page);
    }
    assert_eq!(
        pages
            .iter()
            .map(|page| page.recordings.len())
            .collect::<Vec<_>>(),
        [200, 200, 113]
    );
    assert_eq!(
        pages.iter().map(|page| page.ordinal).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        pages
            .iter()
            .map(|page| page.next_ordinal)
            .collect::<Vec<_>>(),
        [Some(1), Some(2), None]
    );

    let actual = pages
        .iter()
        .flat_map(|page| page.recordings.iter())
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    let identities = actual
        .iter()
        .map(|fact| fact.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(identities.len(), 513);
    for (actual, expected) in actual.iter().zip(expected) {
        fixture::assert_recording(actual, expected, library);
    }
    assert_eq!(actual[0].title, "Boundary 000");
    assert_eq!(actual[1].title, "Boundary 000");
    assert_eq!(actual[63].title, "Boundary 031");
    assert_eq!(actual[64].title, "Boundary 032");
    assert_eq!(actual[127].title, "Boundary 063");
    assert_eq!(actual[128].title, "Boundary 064");
    assert_eq!(actual[255].title, "Boundary 127");
    assert_eq!(actual[256].title, "Boundary 128");
    assert_eq!(actual[511].title, "Boundary 255");
    assert_eq!(actual[512].title, "Boundary 999");
    assert_eq!(
        actual[512].sources[0].release.as_ref().unwrap().entry_id,
        actual[512].sources[0].id
    );
    for private in [
        fixture.dir.path().to_str().unwrap(),
        foreign_library.as_str(),
        fixture.foreign_user_id.as_str(),
        "PRIVATE-CROSS-LIBRARY-SENTINEL",
        "relative_path",
        "provider_id",
    ] {
        assert!(!String::from_utf8_lossy(&serialized).contains(private));
    }
    services.catalog_replication.close().await.unwrap();
    services.catalog_replication.store.pool.close().await;
}

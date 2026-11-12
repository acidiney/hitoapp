use super::fixture::{Fixture, tag_wav, write_wav};
use hito_core::Error;
use std::time::Duration;

#[tokio::test]
async fn repeating_begin_recovers_the_same_snapshot_after_a_lost_response_and_rescan() {
    let f = Fixture::start().await;
    let wav = f.music.join("one.wav");
    write_wav(&wav);
    tag_wav(&wav, "Original", "Artist", "Release", 1, 1);
    f.scan().await;
    let capability = f.services.catalog_replication.clone();
    let actor = f.admin.clone();
    let library = f.library.clone();
    let (ready, response) = tokio::sync::oneshot::channel();
    let observer = tokio::spawn(async move {
        let snapshot = capability.begin(&actor, &library).await.unwrap();
        ready.send(snapshot).unwrap();
        std::future::pending::<()>().await;
    });
    let original = response.await.unwrap();
    observer.abort();
    assert!(observer.await.unwrap_err().is_cancelled());
    let page = f
        .services
        .catalog_replication
        .page(&f.admin, original.id, 0)
        .await
        .unwrap();
    let facts = serde_json::to_value(page).unwrap();
    tag_wav(&wav, "Changed", "Other artist", "Other release", 2, 2);
    f.scan().await;
    let recovered = f
        .services
        .catalog_replication
        .begin(&f.admin, &f.library)
        .await
        .unwrap();
    assert_eq!(recovered, original);
    let page = f
        .services
        .catalog_replication
        .page(&f.admin, recovered.id, 0)
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(page).unwrap(), facts);
    assert!(
        f.services
            .collection
            .events(&f.admin, 0)
            .await
            .unwrap()
            .operations
            .is_empty()
    );
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn library_revocation_allows_own_cleanup_but_a_regrant_never_resumes_the_old_snapshot() {
    let f = Fixture::populated().await;
    let (_, reader) = f.account("reader").await;
    let first = f
        .services
        .catalog_replication
        .begin(&reader, &f.library)
        .await
        .unwrap();
    f.services
        .identity
        .set_library_access(&f.admin, &f.library, &reader.user().id, false)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .begin(&reader, &f.library)
            .await,
        Err(Error::NotFound)
    ));
    f.services
        .catalog_replication
        .release(&reader, first.id)
        .await
        .unwrap();
    f.services
        .identity
        .set_library_access(&f.admin, &f.library, &reader.user().id, true)
        .await
        .unwrap();
    let second = f
        .services
        .catalog_replication
        .begin(&reader, &f.library)
        .await
        .unwrap();
    assert_ne!(first.id, second.id);
    f.services
        .identity
        .set_library_access(&f.admin, &f.library, &reader.user().id, false)
        .await
        .unwrap();
    f.services
        .identity
        .set_library_access(&f.admin, &f.library, &reader.user().id, true)
        .await
        .unwrap();
    let current = f
        .services
        .catalog_replication
        .begin(&reader, &f.library)
        .await
        .unwrap();
    assert_ne!(current.id, second.id);
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&reader, second.id, 0)
            .await,
        Err(Error::NotFound)
    ));
    f.services
        .catalog_replication
        .release(&reader, second.id)
        .await
        .unwrap();
    assert_eq!(
        f.services
            .catalog_replication
            .begin(&reader, &f.library)
            .await
            .unwrap(),
        current
    );
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn a_full_registry_can_resume_without_reserving_capacity_and_only_own_release_frees_a_slot() {
    let f = Fixture::populated().await;
    let original = f
        .services
        .catalog_replication
        .begin(&f.admin, &f.library)
        .await
        .unwrap();
    for account in 1..10 {
        let (_, actor) = f.account(&format!("reader{account}")).await;
        f.services
            .catalog_replication
            .begin(&actor, &f.library)
            .await
            .unwrap();
    }
    let (_, waiting) = f.account("waiting").await;
    assert!(matches!(
        f.services
            .catalog_replication
            .begin(&waiting, &f.library)
            .await,
        Err(Error::Conflict(_))
    ));
    for _ in 0..3 {
        assert_eq!(
            f.services
                .catalog_replication
                .begin(&f.admin, &f.library)
                .await
                .unwrap(),
            original
        );
    }
    f.services
        .catalog_replication
        .release(&waiting, original.id)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .begin(&waiting, &f.library)
            .await,
        Err(Error::Conflict(_))
    ));
    f.services
        .catalog_replication
        .release(&f.admin, original.id)
        .await
        .unwrap();
    assert_eq!(
        f.services
            .catalog_replication
            .begin(&waiting, &f.library)
            .await
            .unwrap()
            .account_id,
        waiting.user().id
    );
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn resuming_does_not_extend_the_original_retention_deadline() {
    let f = Fixture::populated().await;
    let original = f
        .services
        .catalog_replication
        .begin(&f.admin, &f.library)
        .await
        .unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(250)).await;
    tokio::time::resume();
    assert_eq!(
        f.services
            .catalog_replication
            .begin(&f.admin, &f.library)
            .await
            .unwrap(),
        original
    );
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(51)).await;
    tokio::time::resume();
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&f.admin, original.id, 0)
            .await,
        Err(Error::NotFound)
    ));
    f.services
        .catalog_replication
        .release(&f.admin, original.id)
        .await
        .unwrap();
    assert_ne!(
        f.services
            .catalog_replication
            .begin(&f.admin, &f.library)
            .await
            .unwrap()
            .id,
        original.id
    );
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn an_expired_snapshot_can_be_replaced_with_readable_fresh_pages() {
    let f = Fixture::populated().await;
    let original = f
        .services
        .catalog_replication
        .begin(&f.admin, &f.library)
        .await
        .unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(301)).await;
    tokio::time::resume();
    let fresh = f
        .services
        .catalog_replication
        .begin(&f.admin, &f.library)
        .await
        .unwrap();
    assert_ne!(fresh.id, original.id);
    let page = f
        .services
        .catalog_replication
        .page(&f.admin, fresh.id, 0)
        .await
        .unwrap();
    assert_eq!(page.snapshot_id, fresh.id);
    assert_eq!(page.recordings.len(), 1);
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn dropping_an_inflight_observer_keeps_cleanup_owned_and_allows_a_later_begin() {
    use futures_util::FutureExt;
    let f = Fixture::populated().await;
    let mut pending = Box::pin(f.services.catalog_replication.begin(&f.admin, &f.library));
    assert!(pending.as_mut().now_or_never().is_none());
    drop(pending);
    let recovered = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match f
                .services
                .catalog_replication
                .begin(&f.admin, &f.library)
                .await
            {
                Err(Error::Conflict(_)) => tokio::task::yield_now().await,
                result => break result.unwrap(),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        f.services
            .catalog_replication
            .page(&f.admin, recovered.id, 0)
            .await
            .unwrap()
            .recordings
            .len(),
        1
    );
    f.services.catalog_replication.close().await.unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .release(&f.admin, recovered.id)
            .await,
        Err(Error::Internal(_))
    ));
    assert!(f.services.healthy().await);
}

use crate::{Error, test_support::support};
use std::time::Duration;

#[tokio::test]
async fn cancellation_retains_the_byte_reservation_until_inflight_reader_sql_settles() {
    use std::sync::{Arc, mpsc};
    let (_dir, services, actor, library, music) = support::library("catalog-cancel-reader").await;
    support::write_wav(&music.join("owned.wav"), 0);
    support::scan(&services, &actor, &library).await;
    let held = Arc::new(tokio::sync::Notify::new());
    let progress = held.clone();
    let registry = services.catalog_replication.registry.clone();
    let (release, waiting) = mpsc::channel::<()>();
    {
        let mut idle = services.catalog_replication.reader.lock().await;
        let mut stopped = false;
        idle.as_mut()
            .unwrap()
            .lock_handle()
            .await
            .unwrap()
            .set_progress_handler(1, move || {
                if !stopped && registry.lock().unwrap().bytes == super::MAX_EXPORT_BYTES {
                    stopped = true;
                    progress.notify_one();
                    return waiting.recv_timeout(Duration::from_secs(5)).is_ok();
                }
                true
            });
    }
    let capability = services.catalog_replication.clone();
    let owner = actor.clone();
    let selected_library = library.clone();
    let begin = tokio::spawn(async move { capability.begin(&owner, &selected_library).await });
    tokio::time::timeout(Duration::from_secs(3), held.notified())
        .await
        .unwrap();
    assert_eq!(
        services.catalog_replication.registry.lock().unwrap().bytes,
        super::MAX_EXPORT_BYTES
    );
    let completion = services
        .catalog_replication
        .registry
        .lock()
        .unwrap()
        .constructions
        .values()
        .next()
        .unwrap()
        .completion
        .clone();
    begin.abort();
    assert!(begin.await.unwrap_err().is_cancelled());
    tokio::time::timeout(
        Duration::from_secs(3),
        services
            .catalog_replication
            .before_read_settlement
            .notified(),
    )
    .await
    .unwrap();
    assert!(services.catalog_replication.reader.try_lock().is_err());
    assert_eq!(
        services.catalog_replication.registry.lock().unwrap().bytes,
        super::MAX_EXPORT_BYTES,
        "cancelled materialization must retain its reservation while SQLite settlement is blocked"
    );
    release.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), completion)
        .await
        .unwrap();
    assert!(matches!(result, Err(error) if matches!(&*error, Error::Conflict(_))));
    assert_eq!(
        services.catalog_replication.registry.lock().unwrap().bytes,
        0
    );
    services.catalog_replication.close().await.unwrap();
    assert!(services.healthy().await);
}

#[tokio::test]
async fn cancelling_a_held_resume_preserves_the_previously_retained_metadata_and_pages() {
    use futures_util::FutureExt;
    let (_dir, services, actor, library, music) = support::library("catalog-cancel-resume").await;
    support::write_wav(&music.join("owned.wav"), 0);
    support::scan(&services, &actor, &library).await;
    let snapshot = services
        .catalog_replication
        .begin(&actor, &library)
        .await
        .unwrap();
    let page = serde_json::to_value(
        services
            .catalog_replication
            .page(&actor, snapshot.id, 0)
            .await
            .unwrap(),
    )
    .unwrap();
    services
        .catalog_replication
        .before_publication
        .notified()
        .await;
    let retained = services.catalog_replication.registry.lock().unwrap().bytes;
    let writer = services
        .catalog_replication
        .store
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .unwrap();
    let capability = services.catalog_replication.clone();
    let owner = actor.clone();
    let selected_library = library.clone();
    let begin = tokio::spawn(async move { capability.begin(&owner, &selected_library).await });
    tokio::time::timeout(
        Duration::from_secs(3),
        services.catalog_replication.before_publication.notified(),
    )
    .await
    .unwrap();
    assert_eq!(
        services.catalog_replication.registry.lock().unwrap().bytes,
        retained,
        "a resume must not acquire another construction byte reservation"
    );
    let completion = services
        .catalog_replication
        .registry
        .lock()
        .unwrap()
        .constructions
        .values()
        .next()
        .unwrap()
        .completion
        .clone();
    begin.abort();
    assert!(begin.await.unwrap_err().is_cancelled());
    assert!(
        completion.clone().now_or_never().is_none(),
        "the held resume must remain owned until writer SQL settles"
    );
    assert!(services.catalog_replication.reader.try_lock().is_err());
    assert_eq!(
        services
            .catalog_replication
            .constructors
            .available_permits(),
        0
    );
    writer.rollback().await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), completion)
        .await
        .unwrap();
    assert!(matches!(result, Err(error) if matches!(&*error, Error::Conflict(_))));
    assert_eq!(
        services.catalog_replication.registry.lock().unwrap().bytes,
        retained
    );
    assert_eq!(
        services
            .catalog_replication
            .begin(&actor, &library)
            .await
            .unwrap(),
        snapshot
    );
    assert_eq!(
        serde_json::to_value(
            services
                .catalog_replication
                .page(&actor, snapshot.id, 0)
                .await
                .unwrap()
        )
        .unwrap(),
        page
    );
    services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn a_held_publication_writer_cannot_make_an_expired_construction_ready() {
    let (_dir, services, actor, library, music) = support::library("catalog-deadline").await;
    support::write_wav(&music.join("owned.wav"), 0);
    support::scan(&services, &actor, &library).await;
    let pool = &services.catalog_replication.store.pool;
    let mut warm = Vec::new();
    for _ in 0..pool.options().get_max_connections() {
        warm.push(pool.acquire().await.unwrap());
    }
    for connection in &mut warm {
        connection.return_to_pool().await;
    }
    let writer = services
        .catalog_replication
        .store
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .unwrap();
    let capability = services.catalog_replication.clone();
    let owner = actor.clone();
    let selected_library = library.clone();
    let begin = tokio::spawn(async move { capability.begin(&owner, &selected_library).await });
    services
        .catalog_replication
        .before_publication
        .notified()
        .await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(11)).await;
    tokio::time::resume();
    assert!(
        matches!(
            services.catalog_replication.begin(&actor, &library).await,
            Err(Error::Conflict(_))
        ),
        "an in-flight SQL owner must retain constructor admission"
    );
    writer.rollback().await.unwrap();
    let result = begin.await.unwrap();
    assert!(
        matches!(result, Err(Error::Upstream(_))),
        "publication after the construction deadline must fail, got {result:?}"
    );
    services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn an_expired_idle_reader_wait_is_cancelled_without_waiting_for_the_slot() {
    let (_dir, services, actor, library, music) = support::library("catalog-idle-deadline").await;
    support::write_wav(&music.join("owned.wav"), 0);
    support::scan(&services, &actor, &library).await;
    let idle = services.catalog_replication.reader.lock().await;
    let capability = services.catalog_replication.clone();
    let owner = actor.clone();
    let selected_library = library.clone();
    let begin = tokio::spawn(async move { capability.begin(&owner, &selected_library).await });
    services.catalog_replication.before_reader.notified().await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(11)).await;
    tokio::time::resume();
    let result = begin.await.unwrap();
    assert!(
        matches!(result, Err(Error::Upstream(_))),
        "a pure synchronization waiter must time out while its slot is still held, got {result:?}"
    );
    drop(idle);
    let snapshot = services
        .catalog_replication
        .begin(&actor, &library)
        .await
        .unwrap();
    assert_eq!(snapshot.recording_count, 1);
    services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn close_joins_inflight_writer_sql_before_releasing_the_reader() {
    use futures_util::FutureExt;
    let (_dir, services, actor, library, music) = support::library("catalog-close-writer").await;
    support::write_wav(&music.join("owned.wav"), 0);
    support::scan(&services, &actor, &library).await;
    let writer = services
        .catalog_replication
        .store
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .unwrap();
    let capability = services.catalog_replication.clone();
    let begin = tokio::spawn(async move { capability.begin(&actor, &library).await });
    services
        .catalog_replication
        .before_publication
        .notified()
        .await;
    let mut close = Box::pin(services.catalog_replication.close());
    assert!(
        close.as_mut().now_or_never().is_none(),
        "close must wait for retained in-flight SQL"
    );
    assert!(
        services.catalog_replication.reader.try_lock().is_err(),
        "the SQL owner must retain the reader during cancellation cleanup"
    );
    writer.rollback().await.unwrap();
    close.await.unwrap();
    assert!(begin.await.unwrap().is_err());
    assert!(
        services.catalog_replication.reader.lock().await.is_none(),
        "close must gracefully retire the idle SQLite connection after joining its owner"
    );
    assert!(
        services.healthy().await,
        "closing the exporter must preserve other Core capabilities"
    );
}

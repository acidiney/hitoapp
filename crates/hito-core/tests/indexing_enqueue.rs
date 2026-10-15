mod support;

use std::{future::Future, task::Poll, time::Duration};
use support::library;

#[tokio::test]
async fn enqueue_returns_its_job_when_a_worker_finishes_between_awaits() {
    let (_dir, services, actor, library, _music) = library("enqueue-host").await;
    let mut enqueue = std::pin::pin!(services.indexer.enqueue_scan(&actor, &library));
    let mut completed = 0;
    let job = loop {
        let step =
            std::future::poll_fn(|context| Poll::Ready(enqueue.as_mut().poll(context))).await;
        if let Poll::Ready(result) = step {
            break result.expect("A worker completing a scan must not make enqueue fail");
        }
        if let Ok(result) =
            tokio::time::timeout(Duration::from_secs(2), services.indexer.run_next_job()).await
        {
            completed += usize::from(result.unwrap());
        }
        tokio::task::yield_now().await;
    };
    assert_eq!(
        services.indexer.job(&actor, &job.id).await.unwrap().id,
        job.id
    );
    assert_eq!(
        completed, 1,
        "The worker must finish before enqueue returns"
    );
}

#[tokio::test]
async fn repeated_enqueues_share_one_active_scan_and_completed_scans_can_restart() {
    let (_dir, services, actor, library, _music) = library("enqueue-host").await;
    let first = services
        .indexer
        .enqueue_scan(&actor, &library)
        .await
        .unwrap();
    let repeated = services
        .indexer
        .enqueue_scan(&actor, &library)
        .await
        .unwrap();
    assert_eq!(first.id, repeated.id);
    assert!(services.indexer.run_next_job().await.unwrap());
    assert!(!services.indexer.run_next_job().await.unwrap());
    let next = services
        .indexer
        .enqueue_scan(&actor, &library)
        .await
        .unwrap();
    assert_ne!(first.id, next.id);
}

use super::{fixture::Case, peer::Remote};

#[tokio::test]
async fn installed_pages_reopen_offline_with_exact_facts_and_owned_state() {
    let case = Case::new(None).await;
    let remote = Remote::start(401).await;
    let bound = case.bind(&remote).await;
    case.install(&bound).await.unwrap();
    remote.stop().await;
    let case = case.reopen().await;
    let bound = case.activate(bound.connection, &bound.client_id).await;
    case.verify_catalog(&bound, 401).await;
    case.verify_owned_state(&bound).await;
    case.close().await;
}

#[tokio::test]
async fn one_hundred_thousand_records_install_and_reopen_offline() {
    let case = Case::new(None).await;
    let mut remote = Remote::start(100_000).await;
    let bound = case.bind(&remote).await;
    let before = case.disk_usage();
    let started = std::time::Instant::now();
    let installed = case.install(&bound).await.unwrap();
    let elapsed = started.elapsed();
    let after = case.disk_usage();
    let (pages, bytes) = remote.counts();
    assert_eq!(installed.recording_count, 100_000);
    assert_eq!((installed.page_count, pages), (500, 500));
    assert!(bytes <= 128 * 1024 * 1024);
    assert!(elapsed < std::time::Duration::from_secs(120));
    eprintln!(
        "catalog_storage_install case=scale recordings=100000 pages={pages} encoded_bytes={bytes} elapsed_ms={} before={before:?} after={after:?}",
        elapsed.as_millis()
    );
    remote.pause().await;
    let case = case.reopen().await;
    let bound = case.activate(bound.connection, &bound.client_id).await;
    case.verify_catalog(&bound, 100_000).await;
    case.verify_owned_state(&bound).await;
    eprintln!(
        "catalog_storage_reopen case=scale disk={:?}",
        case.disk_usage()
    );
    remote.resume().await;
    remote.assert_pending_and_cursor(&case, &bound).await;
    remote.stop().await;
    case.close().await;
}

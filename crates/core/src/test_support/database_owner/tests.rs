use super::*;
use sqlx::{
    Connection,
    sqlite::{SqliteConnection, SqlitePoolOptions},
};
use std::{future::Future, task::Poll};

fn config(directory: &Path, owner: &DatabaseOwner) -> CoreConfig {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        crate::HostDeviceId::new("owned-database-control").unwrap(),
    );
    owner.attach(&mut config);
    config
}

#[tokio::test]
async fn closing_rejects_late_worker_admission_and_pool_publication() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let owner = DatabaseOwner::retain(directory.clone());
    let options = owner
        .options(
            &directory.path().join("hito.db"),
            SqliteConnectOptions::new()
                .filename(directory.path().join("hito.db"))
                .create_if_missing(true),
        )
        .unwrap_or_else(|_| panic!("owned options failed"));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap_or_else(|_| panic!("owned connection failed"));
    let mut retirement = Box::pin(owner.retire(Duration::from_secs(2)));
    assert!(
        std::future::poll_fn(|context| Poll::Ready(retirement.as_mut().poll(context).is_pending()))
            .await,
        "retirement must wait for the unpublished connection"
    );
    assert!(
        owner.register(&pool).is_err(),
        "closing owner accepted late pool publication"
    );
    let active = owner.observe().active_resources();
    assert!(
        matches!(
            SqliteConnection::connect_with(&options).await,
            Err(sqlx::Error::PoolClosed)
        ),
        "closing owner admitted a new SQLite worker"
    );
    assert!(
        owner.observe().active_resources() == active,
        "late admission created a worker lease"
    );
    pool.close().await;
    let retired = retirement.await;
    assert!(
        retired.resources_retired && !retired.deadline_exceeded,
        "unpublished connection did not retire"
    );
}

#[tokio::test]
async fn failed_core_open_keeps_its_worker_owned_until_retirement() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let weak = Arc::downgrade(&directory);
    let preparation = DatabaseOwner::retain(directory.clone());
    let path = directory.path().join("hito.db");
    let options = preparation
        .options(
            &path,
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .unwrap_or_else(|_| panic!("owned preparation options failed"));
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|_| panic!("owned preparation failed"));
    sqlx::query("CREATE TABLE _sqlx_migrations (invalid INTEGER)")
        .execute(&mut connection)
        .await
        .unwrap_or_else(|_| panic!("owned migration failure fixture failed"));
    connection
        .close()
        .await
        .unwrap_or_else(|_| panic!("owned preparation close failed"));
    assert!(
        preparation
            .retire(Duration::from_secs(2))
            .await
            .resources_retired
    );
    drop(preparation);
    let owner = DatabaseOwner::retain(directory.clone());
    let held = RetirementGate::new();
    owner.hold_retirement(&held);
    assert!(
        crate::open(config(directory.path(), &owner)).await.is_err(),
        "invalid migration fixture unexpectedly opened"
    );
    assert!(
        owner.observe().pool_published(),
        "failed migration must follow registered pool ownership"
    );
    let mut retirement = Box::pin(owner.retire(Duration::from_secs(2)));
    tokio::select! {
        biased;
        _ = &mut retirement => panic!("failed Core opening returned before resource retirement"),
        _ = held.entered() => {}
    }
    assert!(
        owner.observe().active_resources() > 0 && directory.path().exists(),
        "failed opening released worker storage early"
    );
    held.release();
    let retired = retirement.await;
    assert!(
        retired.resources_retired && !retired.deadline_exceeded,
        "failed opening did not retire"
    );
    drop(owner);
    drop(directory);
    assert!(
        weak.upgrade().is_none(),
        "database ownership cycle retained the private directory"
    );
}

#[tokio::test]
async fn checked_out_connection_preserves_deadline_failure_until_it_releases() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let owner = DatabaseOwner::retain(directory.clone());
    let services = crate::open(config(directory.path(), &owner))
        .await
        .unwrap_or_else(|_| panic!("owned Core failed"));
    let held = services
        .store
        .pool
        .acquire()
        .await
        .unwrap_or_else(|_| panic!("owned checked out connection failed"));
    services
        .downloads
        .finish_downloads()
        .await
        .unwrap_or_else(|_| panic!("owned Core jobs failed"));
    drop(services);
    let mut retirement = Box::pin(owner.retire(Duration::from_millis(10)));
    let observation = owner.observe();
    tokio::select! {
        biased;
        _ = &mut retirement => panic!("checked out SQLite connection was not retained"),
        _ = observation.retirement_deadline() => {}
    }
    assert!(
        owner.observe().active_resources() > 0 && directory.path().exists(),
        "deadline released active database ownership"
    );
    let cloned = owner.clone();
    let mut concurrent = Box::pin(cloned.retire(Duration::from_secs(2)));
    assert!(
        std::future::poll_fn(|context| Poll::Ready(concurrent.as_mut().poll(context).is_pending()))
            .await,
        "cloned retirement must wait for the held connection"
    );
    drop(held);
    let (retired, concurrent) = tokio::join!(retirement, concurrent);
    assert!(
        retired.resources_retired && retired.deadline_exceeded,
        "retirement lost its deadline failure after release"
    );
    assert!(
        concurrent.resources_retired && concurrent.deadline_exceeded,
        "cloned retirement masked the owner's deadline failure"
    );
    let repeated = owner.retire(Duration::from_secs(2)).await;
    assert!(
        repeated.resources_retired && repeated.deadline_exceeded,
        "repeated retirement masked the owner's deadline failure"
    );
}

#[tokio::test]
async fn owned_database_rejects_another_directory_and_auxiliary_symlinks() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let other = tempfile::tempdir().unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    assert!(
        crate::open(config(other.path(), &owner)).await.is_err(),
        "owned database accepted another directory"
    );
    assert!(owner.observe().active_resources() == 0 && !owner.observe().pool_published());
    assert!(owner.retire(Duration::from_secs(2)).await.resources_retired);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            other.path().join("outside.sqlite-wal"),
            directory.path().join("hito.db-wal"),
        )
        .unwrap();
        let linked = DatabaseOwner::retain(directory.clone());
        assert!(
            crate::open(config(directory.path(), &linked))
                .await
                .is_err(),
            "owned database accepted an auxiliary symlink"
        );
        assert!(linked.observe().active_resources() == 0 && !linked.observe().pool_published());
        assert!(
            linked
                .retire(Duration::from_secs(2))
                .await
                .resources_retired
        );
    }
}

use super::{
    catalog_fixture::{install, snapshot},
    fixture::{RemoteServer, local},
};
use hito_core::BeginCatalogSnapshot;
use sqlx::Connection;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn repeated_dropped_callers_cancel_requests_and_recover_catalog_admission() {
    let (_directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    let old = Uuid::new_v4();
    let (metadata, pages) = snapshot(old, &["initial"]);
    remote.catalog(metadata, pages);
    install(&services, &actor, &session).await.unwrap();
    let mut current = old;
    for _ in 0..12 {
        let next = Uuid::new_v4();
        let (metadata, pages) = snapshot(next, &["replacement"]);
        remote.catalog(metadata, pages);
        remote.hold_next_catalog_page();
        let replicas = services.replicas.clone();
        let owner = actor.clone();
        let lease = session.clone();
        let caller = tokio::spawn(async move {
            replicas
                .install_catalog(
                    &owner,
                    &lease,
                    BeginCatalogSnapshot {
                        library_id: "remote-library".into(),
                    },
                )
                .await
        });
        remote.wait_for_pull().await;
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        assert_eq!(
            services
                .replicas
                .catalog_snapshot(&actor, &session, "remote-library")
                .await
                .unwrap()
                .unwrap()
                .id,
            current
        );
        remote.release_pull();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match install(&services, &actor, &session).await {
                    Ok(metadata) => {
                        assert_eq!(metadata.id, next);
                        break;
                    }
                    Err(hito_core::Error::Conflict(_)) => {
                        tokio::time::sleep(Duration::from_millis(5)).await
                    }
                    Err(error) => panic!("catalog admission did not recover: {error}"),
                }
            }
        })
        .await
        .unwrap();
        current = next;
    }
    services.replicas.close().await.unwrap();
}

#[tokio::test]
async fn retirement_joins_a_cancelled_blocked_profile_writer_before_returning() {
    let (directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let old = Uuid::new_v4();
    let (metadata, pages) = snapshot(old, &["initial"]);
    remote.catalog(metadata, pages);
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    install(&services, &actor, &session).await.unwrap();
    remote.wait_for_catalog_begin().await;
    let path = directory
        .path()
        .join("hito.replicas")
        .join(connection.id.to_string())
        .join("hito.db");
    let mut blocker = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(path),
    )
    .await
    .unwrap();
    let held = blocker.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let (metadata, pages) = snapshot(Uuid::new_v4(), &["replacement"]);
    remote.catalog(metadata, pages);
    let replicas = services.replicas.clone();
    let owner = actor.clone();
    let lease = session.clone();
    let caller = tokio::spawn(async move {
        replicas
            .install_catalog(
                &owner,
                &lease,
                BeginCatalogSnapshot {
                    library_id: "remote-library".into(),
                },
            )
            .await
    });
    remote.wait_for_catalog_begin().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let replicas = services.replicas.clone();
    let owner = actor.clone();
    let lease = session.clone();
    let retired = tokio::spawn(async move { replicas.deactivate(&owner, &lease).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !retired.is_finished(),
        "retirement returned before the blocked SQL owner settled"
    );
    held.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), retired)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let fresh = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    assert!(
        services
            .replicas
            .catalog_snapshot(&actor, &session, "remote-library")
            .await
            .is_err()
    );
    assert_eq!(
        services
            .replicas
            .catalog_snapshot(&actor, &fresh, "remote-library")
            .await
            .unwrap()
            .unwrap()
            .id,
        old
    );
    services.replicas.close().await.unwrap();
}

#[tokio::test]
async fn deferred_sql_commit_failure_rolls_back_the_new_head_and_admission_recovers() {
    let (directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let old = Uuid::new_v4();
    let (metadata, pages) = snapshot(old, &["initial"]);
    remote.catalog(metadata, pages);
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    install(&services, &actor, &session).await.unwrap();
    let path = directory
        .path()
        .join("hito.replicas")
        .join(connection.id.to_string())
        .join("hito.db");
    let mut fault = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    sqlx::query("CREATE TABLE owned_commit_fault (principal TEXT REFERENCES users(id) DEFERRABLE INITIALLY DEFERRED)").execute(&mut fault).await.unwrap();
    sqlx::query("CREATE TRIGGER owned_head_commit_fault AFTER UPDATE ON replica_catalog_heads BEGIN INSERT INTO owned_commit_fault VALUES('absent-principal'); END").execute(&mut fault).await.unwrap();
    for _ in 0..3 {
        let (metadata, pages) = snapshot(Uuid::new_v4(), &["replacement"]);
        remote.catalog(metadata, pages);
        assert!(install(&services, &actor, &session).await.is_err());
        assert_eq!(
            services
                .replicas
                .catalog_snapshot(&actor, &session, "remote-library")
                .await
                .unwrap()
                .unwrap()
                .id,
            old
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM owned_commit_fault")
            .fetch_one(&mut fault)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let violations: Vec<String> =
            sqlx::query_scalar("SELECT \"table\" FROM pragma_foreign_key_check")
                .fetch_all(&mut fault)
                .await
                .unwrap();
        assert!(violations.is_empty());
    }
    sqlx::query("DROP TRIGGER owned_head_commit_fault")
        .execute(&mut fault)
        .await
        .unwrap();
    sqlx::query("DROP TABLE owned_commit_fault")
        .execute(&mut fault)
        .await
        .unwrap();
    let next = Uuid::new_v4();
    let (metadata, pages) = snapshot(next, &["replacement"]);
    remote.catalog(metadata, pages);
    assert_eq!(install(&services, &actor, &session).await.unwrap().id, next);
    services.replicas.close().await.unwrap();
}

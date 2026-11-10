use super::fixture::{RemoteServer, local};
use std::time::Duration;

#[tokio::test]
async fn closing_the_owner_cancels_and_joins_a_held_remote_pass() {
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
    remote.hold_next_pull();
    let replicas = services.replicas.clone();
    let task = tokio::spawn(async move { replicas.synchronize(&actor, &session).await });
    remote.wait_for_pull().await;
    tokio::time::timeout(Duration::from_secs(1), services.replicas.close())
        .await
        .unwrap()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("the owner must join the held request without its provider responding")
        .unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn a_blocked_profile_write_does_not_lock_control_and_rechecks_retirement_before_commit() {
    use hito_core::{Action, Browse, Operation};
    use sqlx::Connection;
    use uuid::Uuid;
    let (directory, services, actor) = local().await;
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
    let replicas = services.replicas.clone();
    let writer_actor = actor.clone();
    let writer_session = session.clone();
    let task = tokio::spawn(async move {
        replicas
            .enqueue(
                &writer_actor,
                &writer_session,
                Operation {
                    id: Uuid::new_v4(),
                    action: Action::PlaylistCreate {
                        playlist_id: Uuid::new_v4(),
                        name: "Must be retired".into(),
                    },
                },
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let control = tokio::time::timeout(
        Duration::from_millis(500),
        services
            .identity
            .set_display_name(&actor, Some("Independent control change".into())),
    )
    .await;
    let retired = if control.is_ok() {
        Some(services.replicas.deactivate(&actor, &session).await)
    } else {
        None
    };
    held.rollback().await.unwrap();
    let result = task.await.unwrap();
    assert!(
        control.is_ok(),
        "a private profile lock blocked an unrelated control write"
    );
    control.unwrap().unwrap();
    retired.unwrap().unwrap();
    assert!(
        result.is_err(),
        "a retired profile operation committed after its lock was released"
    );
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    assert!(
        services
            .replicas
            .playlists(&actor, &session, Browse::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    services.replicas.close().await.unwrap();
}

#[tokio::test]
async fn retiring_a_lease_cancels_held_requests_before_the_provider_replies() {
    for retirement in 0..3 {
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
        remote.hold_next_pull();
        let replicas = services.replicas.clone();
        let request_actor = actor.clone();
        let request_session = session.clone();
        let mut task =
            tokio::spawn(
                async move { replicas.synchronize(&request_actor, &request_session).await },
            );
        remote.wait_for_pull().await;
        match retirement {
            0 => services
                .replicas
                .deactivate(&actor, &session)
                .await
                .unwrap(),
            1 => services
                .replicas
                .disconnect(&actor, connection.id)
                .await
                .unwrap(),
            _ => {
                let reconnected = services
                    .replicas
                    .connect(&actor, remote.request())
                    .await
                    .unwrap();
                assert_eq!(reconnected.id, connection.id);
                assert_eq!(reconnected.client_id, connection.client_id);
            }
        }
        let result = tokio::time::timeout(Duration::from_secs(1), &mut task).await;
        remote.release_pull();
        if result.is_err() {
            let _ = task.await;
        }
        assert!(
            result.is_ok(),
            "a retired lease retained its provider request"
        );
        assert!(result.unwrap().unwrap().is_err());
        services.replicas.close().await.unwrap();
    }
}

#[tokio::test]
async fn a_session_from_a_closed_owner_is_not_adopted_by_a_reopened_owner() {
    use hito_core::Browse;
    let (directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    let retired = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    services.replicas.close().await.unwrap();
    drop(services);
    let (reopened, fresh_actor) = super::durable::reopen(directory.path()).await;
    let _fresh = reopened
        .replicas
        .activate(&fresh_actor, connection.id)
        .await
        .unwrap();
    assert!(
        reopened
            .replicas
            .playlists(&actor, &retired, Browse::default())
            .await
            .is_err(),
        "the reopened owner adopted an old process-local lease"
    );
    reopened.replicas.close().await.unwrap();
}

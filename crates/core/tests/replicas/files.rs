use super::fixture::{RemoteServer, local};
use hito_core::{Browse, ReplicaConnectionState};

#[cfg(unix)]
#[tokio::test]
async fn an_unsafe_root_is_not_adopted_and_initialization_retries_the_same_binding() {
    let (directory, services, actor) = local().await;
    let outside = tempfile::tempdir().unwrap();
    let sentinel = outside.path().join("user-owned-original");
    std::fs::write(&sentinel, b"preserve this original").unwrap();
    let root = directory.path().join("hito.replicas");
    std::os::unix::fs::symlink(outside.path(), &root).unwrap();
    let remote = RemoteServer::start().await;
    assert!(
        services
            .replicas
            .connect(&actor, remote.request())
            .await
            .is_err()
    );
    let pending = services
        .replicas
        .connections(&actor)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(pending.state, ReplicaConnectionState::Initializing);
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"preserve this original");
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
    services.replicas.close().await.unwrap();
    drop(services);
    let (services, actor) = super::durable::reopen(directory.path()).await;
    std::fs::remove_file(root).unwrap();
    let ready = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    assert_eq!(ready.id, pending.id);
    assert_eq!(ready.client_id, pending.client_id);
    let session = services.replicas.activate(&actor, ready.id).await.unwrap();
    assert_eq!(
        services
            .replicas
            .playlists(&actor, &session, Browse::default())
            .await
            .unwrap()
            .total,
        0
    );
    services.replicas.close().await.unwrap();
}

#[tokio::test]
async fn the_profile_principal_has_no_local_password_session_or_administrator_authority() {
    use sqlx::Connection;
    let (directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    let path = directory
        .path()
        .join("hito.replicas")
        .join(connection.id.to_string())
        .join("hito.db");
    let mut profile = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(path),
    )
    .await
    .unwrap();
    let (account, password, admin): (String, String, bool) =
        sqlx::query_as("SELECT id,password_hash,admin FROM users")
            .fetch_one(&mut profile)
            .await
            .unwrap();
    assert_eq!(account, connection.account.id);
    assert_eq!(password, "!");
    assert!(!admin);
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&mut profile)
        .await
        .unwrap();
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_events")
        .fetch_one(&mut profile)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
    assert_eq!(events, 0);
    profile.close().await.unwrap();
    services.replicas.close().await.unwrap();
}

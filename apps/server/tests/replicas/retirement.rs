use super::fixture::{Host, Library, credentials};
use hito_core::{Action, Browse, Error, Operation};
use uuid::Uuid;

#[tokio::test]
async fn switching_remote_accounts_and_local_sessions_keeps_private_profiles_fenced() {
    let remote = Host::start().await;
    let peer = remote
        .library
        .services
        .identity
        .create_user(&remote.library.actor, credentials("peer"))
        .await
        .unwrap();
    assert!(!peer.admin);
    let local = Library::new("owned-account-switch-client").await;
    let owner_connection = local
        .services
        .replicas
        .connect(&local.actor, remote.request("admin"))
        .await
        .unwrap();
    let retired = local
        .services
        .replicas
        .activate(&local.actor, owner_connection.id)
        .await
        .unwrap();
    local
        .services
        .replicas
        .enqueue(
            &local.actor,
            &retired,
            Operation {
                id: Uuid::new_v4(),
                action: Action::PlaylistCreate {
                    playlist_id: Uuid::new_v4(),
                    name: "Private pending owner playlist".into(),
                },
            },
        )
        .await
        .unwrap();
    let peer_connection = local
        .services
        .replicas
        .connect(&local.actor, remote.request("peer"))
        .await
        .unwrap();
    assert_eq!(peer_connection.account.id, peer.id);
    assert!(!peer_connection.account.admin);
    let peer_session = local
        .services
        .replicas
        .activate(&local.actor, peer_connection.id)
        .await
        .unwrap();
    assert!(
        local
            .services
            .replicas
            .synchronize(&local.actor, &retired)
            .await
            .is_err()
    );
    let synchronized = local
        .services
        .replicas
        .synchronize(&local.actor, &peer_session)
        .await
        .unwrap();
    assert_eq!(synchronized.pushed, 0);
    assert!(
        local
            .services
            .replicas
            .playlists(&local.actor, &peer_session, Browse::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        remote
            .library
            .services
            .collection
            .playlists(&remote.library.actor)
            .await
            .unwrap()
            .is_empty()
    );
    let owner_session = local
        .services
        .replicas
        .activate(&local.actor, owner_connection.id)
        .await
        .unwrap();
    assert!(
        local
            .services
            .replicas
            .playlists(&local.actor, &peer_session, Browse::default())
            .await
            .is_err()
    );
    assert_eq!(
        local
            .services
            .replicas
            .synchronize(&local.actor, &owner_session)
            .await
            .unwrap()
            .pushed,
        1
    );
    let peer_auth = remote
        .library
        .services
        .identity
        .login(credentials("peer"))
        .await
        .unwrap();
    let peer_actor = remote
        .library
        .services
        .identity
        .authenticate(&peer_auth.token)
        .await
        .unwrap();
    assert!(
        remote
            .library
            .services
            .collection
            .playlists(&peer_actor)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        remote
            .library
            .services
            .collection
            .playlists(&remote.library.actor)
            .await
            .unwrap()
            .len(),
        1
    );
    local.services.identity.logout(&local.token).await.unwrap();
    assert!(matches!(
        local
            .services
            .replicas
            .playlists(&local.actor, &owner_session, Browse::default())
            .await,
        Err(Error::Unauthorized)
    ));
    let fresh = local
        .services
        .identity
        .login(credentials("admin"))
        .await
        .unwrap();
    let fresh_actor = local
        .services
        .identity
        .authenticate(&fresh.token)
        .await
        .unwrap();
    assert!(matches!(
        local
            .services
            .replicas
            .playlists(&fresh_actor, &owner_session, Browse::default())
            .await,
        Err(Error::Forbidden)
    ));
    let fresh_session = local
        .services
        .replicas
        .activate(&fresh_actor, owner_connection.id)
        .await
        .unwrap();
    assert_eq!(
        local
            .services
            .replicas
            .playlists(&fresh_actor, &fresh_session, Browse::default())
            .await
            .unwrap()
            .total,
        1
    );
    local.services.replicas.close().await.unwrap();
    remote.close().await;
}

#[tokio::test]
async fn revoked_real_server_session_preserves_outbox_until_explicit_reauthentication() {
    let remote = Host::start().await;
    let local = Library::new("owned-revoked-session-client").await;
    let connection = local
        .services
        .replicas
        .connect(&local.actor, remote.request("admin"))
        .await
        .unwrap();
    let session = local
        .services
        .replicas
        .activate(&local.actor, connection.id)
        .await
        .unwrap();
    let operation = Uuid::new_v4();
    local
        .services
        .replicas
        .enqueue(
            &local.actor,
            &session,
            Operation {
                id: operation,
                action: Action::PlaylistCreate {
                    playlist_id: Uuid::new_v4(),
                    name: "Retained private operation".into(),
                },
            },
        )
        .await
        .unwrap();
    remote
        .library
        .services
        .identity
        .logout(&remote.faults.last_token())
        .await
        .unwrap();
    assert!(matches!(
        local
            .services
            .replicas
            .synchronize(&local.actor, &session)
            .await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert_eq!(
        local
            .services
            .replicas
            .playlists(&local.actor, &session, Browse::default())
            .await
            .unwrap()
            .total,
        1
    );
    assert!(
        remote
            .library
            .services
            .collection
            .playlists(&remote.library.actor)
            .await
            .unwrap()
            .is_empty()
    );
    let reauthenticated = local
        .services
        .replicas
        .connect(&local.actor, remote.request("admin"))
        .await
        .unwrap();
    assert_eq!(reauthenticated.id, connection.id);
    assert_eq!(reauthenticated.client_id, connection.client_id);
    assert!(
        local
            .services
            .replicas
            .synchronize(&local.actor, &session)
            .await
            .is_err()
    );
    let session = local
        .services
        .replicas
        .activate(&local.actor, connection.id)
        .await
        .unwrap();
    let result = local
        .services
        .replicas
        .synchronize(&local.actor, &session)
        .await
        .unwrap();
    assert_eq!(result.pushed, 1);
    assert_eq!(result.pending, 0);
    let events = remote
        .library
        .services
        .collection
        .events(&remote.library.actor, 0)
        .await
        .unwrap();
    assert_eq!(events.operations.len(), 1);
    match &events.operations[0] {
        hito_core::SyncEvent::Operation {
            operation: accepted,
            ..
        } => assert_eq!(accepted.id, operation),
        _ => panic!("unexpected publication"),
    }
    local.services.replicas.close().await.unwrap();
    remote.close().await;
}

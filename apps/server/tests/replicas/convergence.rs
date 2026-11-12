use super::fixture::{Host, Library};
use hito_core::{Action, Browse, Operation, ReplicaSession, SyncEvent};
use uuid::Uuid;

async fn connect(local: &Library, remote: &Host) -> (hito_core::ReplicaConnection, ReplicaSession) {
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
    (connection, session)
}

async fn enqueue(local: &Library, session: &ReplicaSession, action: Action) -> Uuid {
    let id = Uuid::new_v4();
    assert_eq!(
        local
            .services
            .replicas
            .enqueue(&local.actor, session, Operation { id, action })
            .await
            .unwrap()
            .operation_id,
        id
    );
    id
}

async fn sync(local: &Library, session: &ReplicaSession) {
    let result = local
        .services
        .replicas
        .synchronize(&local.actor, session)
        .await
        .unwrap();
    assert_eq!(result.pending, 0);
    assert!(!result.has_more);
}

async fn names(local: &Library, session: &ReplicaSession) -> Vec<String> {
    local
        .services
        .replicas
        .playlists(&local.actor, session, Browse::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|playlist| playlist.name)
        .collect()
}

#[tokio::test]
async fn independent_replicas_retry_exact_durable_operations_and_converge_in_server_order() {
    let remote = Host::start().await;
    let first = Library::new("owned-replica-first").await;
    let second = Library::new("owned-replica-second").await;
    let (connection, first_session) = connect(&first, &remote).await;
    let (peer_connection, second_session) = connect(&second, &remote).await;
    assert_ne!(connection.client_id, peer_connection.client_id);
    assert_ne!(connection.id, peer_connection.id);
    assert_ne!(first.directory.path(), second.directory.path());
    let playlist = Uuid::new_v4();
    let created = enqueue(
        &first,
        &first_session,
        Action::PlaylistCreate {
            playlist_id: playlist,
            name: "Original offline playlist".into(),
        },
    )
    .await;
    remote.faults.lose_next_ack();
    assert!(matches!(
        first
            .services
            .replicas
            .synchronize(&first.actor, &first_session)
            .await,
        Err(hito_core::Error::Upstream(_))
    ));
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
    assert_eq!(
        remote
            .library
            .services
            .collection
            .events(&remote.library.actor, 0)
            .await
            .unwrap()
            .operations
            .len(),
        1
    );
    let first = first.reopen().await;
    let restored = first
        .services
        .replicas
        .connections(&first.actor)
        .await
        .unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, connection.id);
    assert_eq!(restored[0].client_id, connection.client_id);
    assert!(
        first
            .services
            .replicas
            .playlists(&first.actor, &first_session, Browse::default())
            .await
            .is_err()
    );
    let first_session = first
        .services
        .replicas
        .activate(&first.actor, connection.id)
        .await
        .unwrap();
    assert_eq!(
        names(&first, &first_session).await,
        vec!["Original offline playlist"]
    );
    sync(&first, &first_session).await;
    let requests = remote.faults.pushes();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0], requests[1],
        "a lost acknowledgement must replay the exact request after restart"
    );
    sync(&second, &second_session).await;
    assert_eq!(
        names(&first, &first_session).await,
        names(&second, &second_session).await
    );
    let earlier = enqueue(
        &first,
        &first_session,
        Action::PlaylistRename {
            playlist_id: playlist,
            name: "Earlier server acceptance".into(),
        },
    )
    .await;
    let later = enqueue(
        &second,
        &second_session,
        Action::PlaylistRename {
            playlist_id: playlist,
            name: "Later server acceptance".into(),
        },
    )
    .await;
    sync(&first, &first_session).await;
    sync(&second, &second_session).await;
    sync(&first, &first_session).await;
    for (local, session) in [(&first, &first_session), (&second, &second_session)] {
        assert_eq!(names(local, session).await, vec!["Later server acceptance"]);
        assert!(
            local
                .services
                .collection
                .playlists(&local.actor)
                .await
                .unwrap()
                .is_empty()
        );
    }
    let deleted = enqueue(
        &first,
        &first_session,
        Action::PlaylistDelete {
            playlist_id: playlist,
        },
    )
    .await;
    let stale_edit = enqueue(
        &second,
        &second_session,
        Action::PlaylistRename {
            playlist_id: playlist,
            name: "Cannot resurrect a deletion".into(),
        },
    )
    .await;
    sync(&first, &first_session).await;
    sync(&second, &second_session).await;
    sync(&first, &first_session).await;
    assert!(names(&first, &first_session).await.is_empty());
    assert!(names(&second, &second_session).await.is_empty());
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
    let events = remote
        .library
        .services
        .collection
        .events(&remote.library.actor, 0)
        .await
        .unwrap();
    let ids = events
        .operations
        .iter()
        .map(|event| match event {
            SyncEvent::Operation { operation, .. } => operation.id,
            _ => panic!("empty playlist synchronization generated a publication"),
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![created, earlier, later, deleted, stale_edit]);
    first.services.replicas.close().await.unwrap();
    second.services.replicas.close().await.unwrap();
    remote.close().await;
}

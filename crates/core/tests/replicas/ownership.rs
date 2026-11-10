use super::fixture::{REMOTE_ACCOUNT, RemoteServer, local};
use hito_core::{Action, Browse, Operation, ReplicaConnectionState, SyncBatch};
use uuid::Uuid;

#[tokio::test]
async fn connecting_an_owned_remote_profile_preserves_the_standalone_library() {
    let (directory, services, actor) = local().await;
    services
        .collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "standalone-client".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action: Action::PlaylistCreate {
                        playlist_id: Uuid::new_v4(),
                        name: "Standalone list".into(),
                    },
                }],
            },
        )
        .await
        .unwrap();
    let remote = RemoteServer::start().await;
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    assert_eq!(connection.account.id, REMOTE_ACCOUNT);
    assert_eq!(connection.state, ReplicaConnectionState::Ready);
    assert!(!connection.client_id.as_str().is_empty());
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    let page = services
        .replicas
        .playlists(&actor, &session, Browse::default())
        .await
        .unwrap();
    assert_eq!(page.total, 0);
    assert!(page.items.is_empty());
    assert_eq!(
        services.collection.playlists(&actor).await.unwrap()[0].name,
        "Standalone list"
    );
    assert!(
        directory
            .path()
            .join("hito.replicas")
            .join(connection.id.to_string())
            .join("hito.db")
            .is_file()
    );
    assert_eq!(
        services
            .collection
            .events(&actor, 0)
            .await
            .unwrap()
            .operations
            .len(),
        1
    );
}

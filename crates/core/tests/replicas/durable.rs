use super::fixture::{RemoteServer, local};
use hito_core::{
    Action, Browse, CoreConfig, Credentials, Error, HostDeviceId, Operation, Services,
};
use uuid::Uuid;

pub async fn reopen(directory: &std::path::Path) -> (Services, hito_core::Actor) {
    let config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("replica-local-host").unwrap(),
    );
    let services = hito_core::open(config).await.unwrap();
    let authenticated = services
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = services
        .identity
        .authenticate(&authenticated.token)
        .await
        .unwrap();
    (services, actor)
}

#[tokio::test]
async fn offline_changes_survive_reopen_without_replacing_their_identity() {
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
    let playlist = Uuid::new_v4();
    let create_id = Uuid::new_v4();
    let create = || Operation {
        id: create_id,
        action: Action::PlaylistCreate {
            playlist_id: playlist,
            name: "Offline list".into(),
        },
    };
    services
        .replicas
        .enqueue(&actor, &session, create())
        .await
        .unwrap();
    services
        .replicas
        .enqueue(
            &actor,
            &session,
            Operation {
                id: Uuid::new_v4(),
                action: Action::PlaylistRename {
                    playlist_id: playlist,
                    name: "Renamed offline".into(),
                },
            },
        )
        .await
        .unwrap();
    services.replicas.close().await.unwrap();
    drop(services);
    drop(remote);
    let (services, actor) = reopen(directory.path()).await;
    let connections = services.replicas.connections(&actor).await.unwrap();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].id, connection.id);
    assert_eq!(connections[0].client_id, connection.client_id);
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    let replay = services
        .replicas
        .enqueue(&actor, &session, create())
        .await
        .unwrap();
    assert_eq!(replay.operation_id, create_id);
    let changed = services
        .replicas
        .enqueue(
            &actor,
            &session,
            Operation {
                id: create_id,
                action: Action::PlaylistCreate {
                    playlist_id: playlist,
                    name: "Changed request".into(),
                },
            },
        )
        .await;
    assert!(matches!(changed, Err(Error::Conflict(_))));
    let page = services
        .replicas
        .playlists(&actor, &session, Browse::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].name, "Renamed offline");
    assert!(
        services
            .collection
            .playlists(&actor)
            .await
            .unwrap()
            .is_empty()
    );
    services.replicas.close().await.unwrap();
}

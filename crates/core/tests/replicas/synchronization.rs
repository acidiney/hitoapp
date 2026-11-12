use super::fixture::{RemoteServer, local};
use hito_core::{Action, Browse, Operation};
use uuid::Uuid;

#[tokio::test]
async fn accepted_changes_reconcile_once_and_the_following_pass_is_empty() {
    let (_directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    let lease = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    let playlist = Uuid::new_v4();
    for action in [
        Action::PlaylistCreate {
            playlist_id: playlist,
            name: "Offline list".into(),
        },
        Action::PlaylistRename {
            playlist_id: playlist,
            name: "Accepted list".into(),
        },
    ] {
        services
            .replicas
            .enqueue(
                &actor,
                &lease,
                Operation {
                    id: Uuid::new_v4(),
                    action,
                },
            )
            .await
            .unwrap();
    }
    let first = services.replicas.synchronize(&actor, &lease).await.unwrap();
    assert_eq!(first.pushed, 2);
    assert_eq!(first.received, 2);
    assert_eq!(first.cursor, 2);
    assert_eq!(first.pending, 0);
    assert!(!first.has_more);
    let page = services
        .replicas
        .playlists(&actor, &lease, Browse::default())
        .await
        .unwrap();
    assert_eq!(page.items[0].name, "Accepted list");
    let next = services.replicas.synchronize(&actor, &lease).await.unwrap();
    assert_eq!(next.pushed, 0);
    assert_eq!(next.received, 0);
    assert_eq!(next.cursor, 2);
    assert_eq!(next.pending, 0);
    assert!(
        services
            .collection
            .events(&actor, 0)
            .await
            .unwrap()
            .operations
            .is_empty()
    );
    services.replicas.close().await.unwrap();
}

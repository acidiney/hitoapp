mod support;

use hito_core::{Action, Error, Operation, SyncBatch};
use support::library;
use uuid::Uuid;

fn create(id: Uuid, playlist: Uuid) -> Operation {
    Operation {
        id,
        action: Action::PlaylistCreate {
            playlist_id: playlist,
            name: "Retry safely".into(),
        },
    }
}

#[tokio::test]
async fn failed_batch_rolls_back_state_and_log_and_can_retry_the_same_operation_id() {
    let (_dir, services, actor, _library, _music) = library("local-backend").await;
    let playlist = Uuid::new_v4();
    let operation = Uuid::new_v4();
    let result = services
        .collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "offline-origin".into(),
                operations: vec![
                    create(operation, playlist),
                    Operation {
                        id: Uuid::new_v4(),
                        action: Action::PlaylistRename {
                            playlist_id: Uuid::new_v4(),
                            name: "Missing".into(),
                        },
                    },
                ],
            },
        )
        .await;
    assert!(matches!(result, Err(Error::NotFound)));
    assert!(
        services
            .collection
            .playlists(&actor)
            .await
            .unwrap()
            .is_empty()
    );
    let page = services.collection.events(&actor, 0).await.unwrap();
    assert!(page.operations.is_empty());
    assert_eq!(page.cursor, 0);

    for _ in 0..2 {
        let receipt = services
            .collection
            .apply(
                &actor,
                SyncBatch {
                    device_id: "offline-origin".into(),
                    operations: vec![create(operation, playlist)],
                },
            )
            .await
            .unwrap();
        assert_eq!(receipt.acknowledged, vec![operation]);
    }
    assert_eq!(
        services.collection.playlists(&actor).await.unwrap().len(),
        1
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

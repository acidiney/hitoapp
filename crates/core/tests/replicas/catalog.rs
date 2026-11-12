use super::{
    catalog_fixture::snapshot,
    fixture::{RemoteServer, local},
};
use hito_core::{Action, BeginCatalogSnapshot, Browse, Operation, SyncBatch};
use uuid::Uuid;

#[tokio::test]
async fn installed_catalog_survives_restart_offline_without_touching_collection_or_standalone() {
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
                        name: "Standalone".into(),
                    },
                }],
            },
        )
        .await
        .unwrap();
    let remote = RemoteServer::start().await;
    let id = Uuid::new_v4();
    let (metadata, pages) = snapshot(id, &["", "legacy-recording"]);
    remote.catalog(metadata.clone(), pages.clone());
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
    let operation = || Operation {
        id: Uuid::from_u128(7),
        action: Action::PlaylistCreate {
            playlist_id: Uuid::from_u128(8),
            name: "Offline".into(),
        },
    };
    services
        .replicas
        .enqueue(&actor, &session, operation())
        .await
        .unwrap();
    let installed = services
        .replicas
        .install_catalog(
            &actor,
            &session,
            BeginCatalogSnapshot {
                library_id: "remote-library".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(installed).unwrap(), metadata);
    let synchronized = services
        .replicas
        .synchronize(&actor, &session)
        .await
        .unwrap();
    assert_eq!(synchronized.cursor, 1);
    assert_eq!(synchronized.pushed, 1);
    services
        .replicas
        .enqueue(
            &actor,
            &session,
            Operation {
                id: Uuid::from_u128(9),
                action: Action::PlaylistRename {
                    playlist_id: Uuid::from_u128(8),
                    name: "Pending rename".into(),
                },
            },
        )
        .await
        .unwrap();
    services.replicas.close().await.unwrap();
    drop(services);
    drop(remote);
    let (services, actor) = super::durable::reopen(directory.path()).await;
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    let cached = services
        .replicas
        .catalog_snapshot(&actor, &session, "remote-library")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(serde_json::to_value(cached).unwrap(), metadata);
    for (ordinal, expected) in pages.into_iter().enumerate() {
        let actual = services
            .replicas
            .catalog_page(&actor, &session, id, ordinal as u64)
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(actual).unwrap(), expected);
    }
    services
        .replicas
        .enqueue(&actor, &session, operation())
        .await
        .unwrap();
    assert_eq!(
        services
            .replicas
            .playlists(&actor, &session, Browse::default())
            .await
            .unwrap()
            .items[0]
            .name,
        "Pending rename"
    );
    assert_eq!(
        services.collection.playlists(&actor).await.unwrap()[0].name,
        "Standalone"
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
    assert_eq!(
        services
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    services.replicas.close().await.unwrap();
}

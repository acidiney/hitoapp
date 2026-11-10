use super::catalog_fixture::{cached_pages, create_playlist, multipage_snapshot};
use super::fixture::{Host, Library};
use hito_core::{
    Action, BeginCatalogSnapshot, Browse, Operation, Search, SyncBatch, SyncEvent,
    test_support::support::{scan, write_wav},
};
use uuid::Uuid;

#[tokio::test]
async fn installed_catalog_survives_offline_restart_without_acknowledging_pending_collection_work()
{
    let remote = Host::start().await;
    let (server_library, expected, expected_pages) = multipage_snapshot(&remote).await;

    let local = Library::new("owned-offline-catalog-client").await;
    let original_path = local.directory.path().join("music/standalone.wav");
    let original = write_wav(&original_path, 1901);
    let standalone_library = local
        .services
        .catalog
        .libraries(&local.actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    scan(&local.services, &local.actor, &standalone_library).await;
    let standalone_playlist = create_playlist(&local, "Standalone remains separate").await;
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
    let pending_id = Uuid::new_v4();
    let pending_playlist = Uuid::new_v4();
    let pending = || Operation {
        id: pending_id,
        action: Action::PlaylistCreate {
            playlist_id: pending_playlist,
            name: "Retained offline catalog work".into(),
        },
    };
    local
        .services
        .replicas
        .enqueue(&local.actor, &session, pending())
        .await
        .unwrap();
    let installed = local
        .services
        .replicas
        .install_catalog(
            &local.actor,
            &session,
            BeginCatalogSnapshot {
                library_id: server_library.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(installed, expected);
    assert_eq!(
        cached_pages(&local, &session, &installed).await,
        expected_pages
    );
    assert!(remote.faults.pushes().is_empty());

    let closed_local = local.close().await;
    let closed_remote = remote.stop().await;
    let local = closed_local.open().await;
    assert!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &session, &server_library)
            .await
            .is_err()
    );
    assert!(
        local
            .services
            .replicas
            .install_catalog(
                &local.actor,
                &session,
                BeginCatalogSnapshot {
                    library_id: server_library.clone()
                }
            )
            .await
            .is_err()
    );
    let restored = local
        .services
        .replicas
        .connections(&local.actor)
        .await
        .unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, connection.id);
    assert_eq!(restored[0].client_id, connection.client_id);
    let session = local
        .services
        .replicas
        .activate(&local.actor, connection.id)
        .await
        .unwrap();
    assert_eq!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &session, &server_library)
            .await
            .unwrap(),
        Some(expected.clone())
    );
    assert_eq!(
        cached_pages(&local, &session, &installed).await,
        expected_pages
    );
    assert!(
        local
            .services
            .replicas
            .install_catalog(
                &local.actor,
                &session,
                BeginCatalogSnapshot {
                    library_id: server_library.clone()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        cached_pages(&local, &session, &installed).await,
        expected_pages
    );
    assert_eq!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &session, &server_library)
            .await
            .unwrap(),
        Some(expected.clone())
    );
    assert_eq!(
        local
            .services
            .replicas
            .enqueue(&local.actor, &session, pending())
            .await
            .unwrap()
            .operation_id,
        pending_id
    );
    let projected = local
        .services
        .replicas
        .playlists(&local.actor, &session, Browse::default())
        .await
        .unwrap();
    assert_eq!(projected.total, 1);
    assert_eq!(projected.items[0].id, pending_playlist.to_string());
    assert_eq!(projected.items[0].name, "Retained offline catalog work");
    let standalone = local
        .services
        .collection
        .playlists(&local.actor)
        .await
        .unwrap();
    assert_eq!(standalone.len(), 1);
    assert_eq!(standalone[0].id, standalone_playlist.to_string());
    let recordings = local
        .services
        .catalog
        .recordings(&local.actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert_eq!(recordings.items[0].library_id, standalone_library);
    assert_eq!(std::fs::read(&original_path).unwrap(), original);

    let remote = closed_remote.start().await;
    let result = local
        .services
        .replicas
        .synchronize(&local.actor, &session)
        .await
        .unwrap();
    assert_eq!(result.pushed, 1);
    assert_eq!(
        result.received, 2,
        "Snapshot watermark must not advance the collection cursor"
    );
    assert_eq!(result.cursor, 2);
    assert_eq!(result.pending, 0);
    assert!(!result.has_more);
    let pushes = remote.faults.pushes();
    assert_eq!(pushes.len(), 1);
    let sent: SyncBatch = serde_json::from_slice(&pushes[0]).unwrap();
    assert_eq!(sent.device_id, connection.client_id.as_str());
    assert_eq!(sent.operations.len(), 1);
    assert_eq!(
        serde_json::to_value(&sent.operations[0]).unwrap(),
        serde_json::to_value(pending()).unwrap()
    );
    let repeated = local
        .services
        .replicas
        .synchronize(&local.actor, &session)
        .await
        .unwrap();
    assert_eq!(repeated.pushed, 0);
    assert_eq!(repeated.received, 0);
    let accepted = remote
        .library
        .services
        .collection
        .events(&remote.library.actor, 0)
        .await
        .unwrap();
    assert_eq!(accepted.operations.len(), 2);
    assert_eq!(accepted.operations.iter().filter(|event| matches!(event, SyncEvent::Operation { operation, .. } if operation.id == pending_id)).count(), 1);
    assert_eq!(
        cached_pages(&local, &session, &installed).await,
        expected_pages
    );
    assert_eq!(std::fs::read(original_path).unwrap(), original);
    drop(local.close().await);
    remote.close().await;
}

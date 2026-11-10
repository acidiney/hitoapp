use super::catalog_fixture::{cached_pages, tag_wav};
use super::fixture::{Host, Library, credentials};
use hito_core::{
    BeginCatalogSnapshot, Error,
    test_support::support::{scan, write_wav},
};

#[tokio::test]
async fn authorized_refresh_replaces_facts_and_failed_remote_authority_preserves_the_committed_cache()
 {
    let remote = Host::start().await;
    let library = remote
        .library
        .services
        .catalog
        .libraries(&remote.library.actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    let path = remote
        .library
        .directory
        .path()
        .join("music/owned-refresh.wav");
    write_wav(&path, 83);
    tag_wav(&path, "Original catalog title");
    scan(&remote.library.services, &remote.library.actor, &library).await;
    let reader = remote
        .library
        .services
        .identity
        .create_user(&remote.library.actor, credentials("reader"))
        .await
        .unwrap();
    remote
        .library
        .services
        .identity
        .set_library_access(&remote.library.actor, &library, &reader.id, true)
        .await
        .unwrap();
    let local = Library::new("owned-catalog-refresh-client").await;
    let connection = local
        .services
        .replicas
        .connect(&local.actor, remote.request("reader"))
        .await
        .unwrap();
    let session = local
        .services
        .replicas
        .activate(&local.actor, connection.id)
        .await
        .unwrap();
    let request = || BeginCatalogSnapshot {
        library_id: library.clone(),
    };
    let original = local
        .services
        .replicas
        .install_catalog(&local.actor, &session, request())
        .await
        .unwrap();
    assert_eq!(original.account_id, reader.id);
    let original_pages = cached_pages(&local, &session, &original).await;
    assert_eq!(
        original_pages[0]["recordings"][0]["title"],
        "Original catalog title"
    );
    let remote_actor = remote
        .library
        .services
        .identity
        .authenticate(&remote.faults.last_token())
        .await
        .unwrap();
    assert!(
        matches!(
            remote
                .library
                .services
                .catalog_replication
                .page(&remote_actor, original.id, 0)
                .await,
            Err(Error::NotFound)
        ),
        "Successful installation must release the producer handle"
    );

    tag_wav(&path, "Refreshed catalog title");
    scan(&remote.library.services, &remote.library.actor, &library).await;
    remote
        .library
        .services
        .identity
        .set_library_access(&remote.library.actor, &library, &reader.id, false)
        .await
        .unwrap();
    assert!(matches!(
        local
            .services
            .replicas
            .install_catalog(&local.actor, &session, request())
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &session, &library)
            .await
            .unwrap(),
        Some(original.clone())
    );
    assert_eq!(
        cached_pages(&local, &session, &original).await,
        original_pages
    );

    remote
        .library
        .services
        .identity
        .set_library_access(&remote.library.actor, &library, &reader.id, true)
        .await
        .unwrap();
    let refreshed = local
        .services
        .replicas
        .install_catalog(&local.actor, &session, request())
        .await
        .unwrap();
    assert_ne!(refreshed.id, original.id);
    let refreshed_pages = cached_pages(&local, &session, &refreshed).await;
    let before = &original_pages[0]["recordings"][0];
    let after = &refreshed_pages[0]["recordings"][0];
    assert_eq!(after["id"], before["id"]);
    assert_eq!(after["sources"][0]["id"], before["sources"][0]["id"]);
    assert_eq!(
        after["sources"][0]["release"],
        before["sources"][0]["release"]
    );
    assert_eq!(after["title"], "Refreshed catalog title");
    assert_eq!(after["sources"][0]["title"], "Refreshed catalog title");
    assert_eq!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &session, &library)
            .await
            .unwrap(),
        Some(refreshed.clone())
    );
    assert!(matches!(
        local
            .services
            .replicas
            .catalog_page(&local.actor, &session, original.id, 0)
            .await,
        Err(Error::NotFound)
    ));

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
            .install_catalog(&local.actor, &session, request())
            .await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert_eq!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &session, &library)
            .await
            .unwrap(),
        Some(refreshed.clone())
    );
    assert_eq!(
        cached_pages(&local, &session, &refreshed).await,
        refreshed_pages
    );
    let reauthenticated = local
        .services
        .replicas
        .connect(&local.actor, remote.request("reader"))
        .await
        .unwrap();
    assert_eq!(reauthenticated.id, connection.id);
    assert_eq!(reauthenticated.client_id, connection.client_id);
    assert!(matches!(
        local
            .services
            .replicas
            .install_catalog(&local.actor, &session, request())
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        local
            .services
            .replicas
            .catalog_page(&local.actor, &session, refreshed.id, 0)
            .await,
        Err(Error::Conflict(_))
    ));
    let fresh_session = local
        .services
        .replicas
        .activate(&local.actor, connection.id)
        .await
        .unwrap();
    assert_eq!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &fresh_session, &library)
            .await
            .unwrap(),
        Some(refreshed)
    );
    assert_eq!(remote.faults.pushes().len(), 0);
    drop(local.close().await);
    remote.close().await;
}

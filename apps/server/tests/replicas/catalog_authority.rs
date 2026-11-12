use super::catalog_fixture::{cached_pages, tag_wav};
use super::fixture::{Host, Library, PASSWORD, credentials};
use hito_core::{
    BeginCatalogSnapshot, Error, SetPin, Unlock,
    test_support::support::{scan, write_wav},
};

#[tokio::test]
async fn local_pin_and_account_boundaries_keep_catalog_profiles_private() {
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
        .join("music/owned-private.wav");
    write_wav(&path, 91);
    tag_wav(&path, "Private catalog title");
    scan(&remote.library.services, &remote.library.actor, &library).await;
    let foreign = remote
        .library
        .services
        .identity
        .create_user(&remote.library.actor, credentials("foreign"))
        .await
        .unwrap();
    remote
        .library
        .services
        .identity
        .set_library_access(&remote.library.actor, &library, &foreign.id, false)
        .await
        .unwrap();
    let local = Library::new("owned-catalog-authority-client").await;
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
    let request = || BeginCatalogSnapshot {
        library_id: library.clone(),
    };
    let snapshot = local
        .services
        .replicas
        .install_catalog(&local.actor, &session, request())
        .await
        .unwrap();
    let pages = cached_pages(&local, &session, &snapshot).await;

    local
        .services
        .identity
        .set_pin(
            &local.actor,
            SetPin {
                password: PASSWORD.into(),
                pin: Some("1234".into()),
                idle_minutes: 0,
            },
        )
        .await
        .unwrap();
    local.services.identity.lock(&local.actor).await.unwrap();
    assert!(matches!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &session, &library)
            .await,
        Err(Error::Locked)
    ));
    assert!(matches!(
        local
            .services
            .replicas
            .catalog_page(&local.actor, &session, snapshot.id, 0)
            .await,
        Err(Error::Locked)
    ));
    assert!(matches!(
        local
            .services
            .replicas
            .install_catalog(&local.actor, &session, request())
            .await,
        Err(Error::Locked)
    ));
    local
        .services
        .identity
        .unlock(&local.actor, Unlock { pin: "1234".into() })
        .await
        .unwrap();
    assert_eq!(cached_pages(&local, &session, &snapshot).await, pages);

    local
        .services
        .identity
        .create_user(&local.actor, credentials("peer"))
        .await
        .unwrap();
    let peer_login = local
        .services
        .identity
        .login(credentials("peer"))
        .await
        .unwrap();
    let peer = local
        .services
        .identity
        .authenticate(&peer_login.token)
        .await
        .unwrap();
    assert!(
        local
            .services
            .replicas
            .connections(&peer)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        local
            .services
            .replicas
            .catalog_snapshot(&peer, &session, &library)
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        local
            .services
            .replicas
            .catalog_page(&peer, &session, snapshot.id, 0)
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        local
            .services
            .replicas
            .install_catalog(&peer, &session, request())
            .await,
        Err(Error::Forbidden)
    ));
    let peer_connection = local
        .services
        .replicas
        .connect(&peer, remote.request("admin"))
        .await
        .unwrap();
    assert_ne!(peer_connection.id, connection.id);
    let peer_session = local
        .services
        .replicas
        .activate(&peer, peer_connection.id)
        .await
        .unwrap();
    assert_eq!(
        local
            .services
            .replicas
            .catalog_snapshot(&peer, &peer_session, &library)
            .await
            .unwrap(),
        None
    );
    assert!(matches!(
        local
            .services
            .replicas
            .catalog_page(&peer, &peer_session, snapshot.id, 0)
            .await,
        Err(Error::NotFound)
    ));

    let foreign_connection = local
        .services
        .replicas
        .connect(&local.actor, remote.request("foreign"))
        .await
        .unwrap();
    assert_ne!(foreign_connection.id, connection.id);
    let foreign_session = local
        .services
        .replicas
        .activate(&local.actor, foreign_connection.id)
        .await
        .unwrap();
    assert_eq!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &foreign_session, &library)
            .await
            .unwrap(),
        None
    );
    assert!(matches!(
        local
            .services
            .replicas
            .catalog_page(&local.actor, &foreign_session, snapshot.id, 0)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        local
            .services
            .replicas
            .install_catalog(&local.actor, &foreign_session, request())
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &foreign_session, &library)
            .await
            .unwrap(),
        None
    );
    assert!(matches!(
        local
            .services
            .replicas
            .install_catalog(&local.actor, &session, request())
            .await,
        Err(Error::Conflict(_))
    ));
    let restored_session = local
        .services
        .replicas
        .activate(&local.actor, connection.id)
        .await
        .unwrap();
    assert_eq!(
        local
            .services
            .replicas
            .catalog_snapshot(&local.actor, &restored_session, &library)
            .await
            .unwrap(),
        Some(snapshot.clone())
    );
    assert_eq!(
        cached_pages(&local, &restored_session, &snapshot).await,
        pages
    );
    assert!(remote.faults.pushes().is_empty());
    drop(local.close().await);
    remote.close().await;
}

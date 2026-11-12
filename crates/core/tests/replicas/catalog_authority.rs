use super::{
    catalog_fixture::{install, snapshot},
    fixture::{RemoteServer, local},
};
use hito_core::{Credentials, Error, SetPin, Unlock};
use uuid::Uuid;

#[tokio::test]
async fn cached_catalog_requires_the_exact_current_unlocked_local_session() {
    let (_directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let id = Uuid::new_v4();
    let (metadata, pages) = snapshot(id, &["recording"]);
    remote.catalog(metadata, pages);
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
    install(&services, &actor, &session).await.unwrap();
    let credentials = || Credentials {
        username: "other-reader".into(),
        password: "owned other reader password".into(),
    };
    services
        .identity
        .create_user(&actor, credentials())
        .await
        .unwrap();
    let authenticated = services.identity.login(credentials()).await.unwrap();
    let other = services
        .identity
        .authenticate(&authenticated.token)
        .await
        .unwrap();
    assert!(matches!(
        install(&services, &other, &session).await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        services
            .replicas
            .catalog_snapshot(&other, &session, "remote-library")
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        services
            .replicas
            .catalog_page(&other, &session, id, 0)
            .await,
        Err(Error::Forbidden)
    ));
    services
        .identity
        .set_pin(
            &actor,
            SetPin {
                password: "indexing test password".into(),
                pin: Some("1234".into()),
                idle_minutes: 0,
            },
        )
        .await
        .unwrap();
    services.identity.lock(&actor).await.unwrap();
    assert!(matches!(
        install(&services, &actor, &session).await,
        Err(Error::Locked)
    ));
    assert!(matches!(
        services
            .replicas
            .catalog_snapshot(&actor, &session, "remote-library")
            .await,
        Err(Error::Locked)
    ));
    assert!(matches!(
        services
            .replicas
            .catalog_page(&actor, &session, id, 0)
            .await,
        Err(Error::Locked)
    ));
    services
        .identity
        .unlock(&actor, Unlock { pin: "1234".into() })
        .await
        .unwrap();
    assert_eq!(
        services
            .replicas
            .catalog_page(&actor, &session, id, 0)
            .await
            .unwrap()
            .recordings
            .len(),
        1
    );
    let authenticated = services
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let revoked = services
        .identity
        .authenticate(&authenticated.token)
        .await
        .unwrap();
    services
        .identity
        .unlock(&revoked, Unlock { pin: "1234".into() })
        .await
        .unwrap();
    let revoked_session = services
        .replicas
        .activate(&revoked, connection.id)
        .await
        .unwrap();
    services
        .identity
        .logout(&authenticated.token)
        .await
        .unwrap();
    assert!(matches!(
        services
            .replicas
            .catalog_page(&revoked, &revoked_session, id, 0)
            .await,
        Err(Error::Unauthorized)
    ));
    services.replicas.close().await.unwrap();
}

#[tokio::test]
async fn disconnect_retains_last_known_cache_and_reauthentication_retires_old_generations() {
    let (_directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let old = Uuid::new_v4();
    let (metadata, pages) = snapshot(old, &["recording"]);
    remote.catalog(metadata, pages);
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
    install(&services, &actor, &session).await.unwrap();
    services
        .replicas
        .disconnect(&actor, connection.id)
        .await
        .unwrap();
    assert!(
        services
            .replicas
            .catalog_page(&actor, &session, old, 0)
            .await
            .is_err()
    );
    let offline = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    assert_eq!(
        services
            .replicas
            .catalog_page(&actor, &offline, old, 0)
            .await
            .unwrap()
            .recordings[0]
            .id,
        "recording"
    );
    assert!(matches!(
        install(&services, &actor, &offline).await,
        Err(Error::AuthorizationRequired(_))
    ));
    let reconnected = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    assert_eq!(connection.id, reconnected.id);
    assert_eq!(connection.client_id, reconnected.client_id);
    assert!(
        services
            .replicas
            .catalog_page(&actor, &offline, old, 0)
            .await
            .is_err()
    );
    let current = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    assert_eq!(
        services
            .replicas
            .catalog_snapshot(&actor, &current, "remote-library")
            .await
            .unwrap()
            .unwrap()
            .id,
        old
    );
    let fresh = Uuid::new_v4();
    let (metadata, pages) = snapshot(fresh, &["new-recording"]);
    remote.catalog(metadata, pages);
    install(&services, &actor, &current).await.unwrap();
    assert!(
        services
            .replicas
            .catalog_page(&actor, &current, old, 0)
            .await
            .is_err()
    );
    assert_eq!(
        services
            .replicas
            .catalog_page(&actor, &current, fresh, 0)
            .await
            .unwrap()
            .recordings[0]
            .id,
        "new-recording"
    );
    services.replicas.close().await.unwrap();
}

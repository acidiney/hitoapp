#[path = "fixtures/plugin_service.rs"]
mod fixture;
mod support;

use hito_core::{Credentials, Error, InstallPlugin, OfferPluginService};

#[tokio::test]
async fn an_account_connects_an_offered_service_through_its_real_manifest_endpoint() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugins-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Community provider".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        hito.plugins
            .install(
                &actor,
                InstallPlugin {
                    service_id: service.id.clone(),
                    service_revision: service.revision,
                    bearer_token: None,
                }
            )
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        server.hits(),
        1,
        "duplicate installations do not trigger network requests"
    );
    assert_ne!(installed.id, installed.manifest.id);
    assert_eq!(installed.manifest.id, "org.hito.test");
    assert_eq!(installed.manifest.name, "Fixture provider");
    let settings = hito.plugins.settings(&actor).await.unwrap();
    assert_eq!(settings.services.len(), 1);
    assert_eq!(settings.installations.len(), 1);
    assert_eq!(settings.installations[0].id, installed.id);
    assert_eq!(server.headers().await[0]["x-hito-plugin-protocol"], "1");
    assert!(server.headers().await[0].get("cookie").is_none());
}

#[tokio::test]
async fn personal_installations_are_private_even_when_accounts_share_the_same_service() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, alice, _library, _music) = support::library("plugin-privacy-host").await;
    hito.identity
        .create_user(
            &alice,
            Credentials {
                username: "bob".into(),
                password: "plugin bob password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "bob".into(),
            password: "plugin bob password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    assert!(matches!(
        hito.plugins
            .offer_service(
                &bob,
                OfferPluginService {
                    name: "Not permitted".into(),
                    base_url: server.url.clone(),
                }
            )
            .await,
        Err(Error::Forbidden)
    ));
    let service = hito
        .plugins
        .offer_service(
            &alice,
            OfferPluginService {
                name: "Shared choice".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let alice_install = hito
        .plugins
        .install(
            &alice,
            InstallPlugin {
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    assert!(
        hito.plugins
            .settings(&bob)
            .await
            .unwrap()
            .installations
            .is_empty()
    );
    let bob_install = hito
        .plugins
        .install(
            &bob,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    assert_ne!(alice_install.id, bob_install.id);
    assert!(matches!(
        hito.plugins
            .remove(&bob, &alice_install.id, alice_install.revision)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.plugins
            .remove(&alice, &bob_install.id, bob_install.revision)
            .await,
        Err(Error::NotFound)
    ));
    hito.plugins
        .remove(&bob, &bob_install.id, bob_install.revision)
        .await
        .unwrap();
    assert!(
        hito.plugins
            .settings(&bob)
            .await
            .unwrap()
            .installations
            .is_empty()
    );
    assert_eq!(
        hito.plugins.settings(&alice).await.unwrap().installations[0].id,
        alice_install.id
    );
    assert_eq!(server.hits(), 2);
}

#[path = "fixtures/plugin_service.rs"]
mod fixture;
mod support;

use hito_core::{Credentials, Error, InstallPlugin, OfferPluginService};

#[tokio::test]
async fn an_admin_offers_an_address_using_the_validated_manifest_name() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-automatic-name").await;
    let request: OfferPluginService = serde_json::from_value(serde_json::json!({
        "base_url": server.url.trim_end_matches('/'),
    }))
    .unwrap();
    let service = hito.plugins.offer_service(&actor, request).await.unwrap();

    assert_eq!(service.name, "Fixture provider");
    assert_eq!(service.base_url, server.url);
    assert_eq!(server.hits(), 1);
    assert!(
        hito.plugins
            .settings(&actor)
            .await
            .unwrap()
            .installations
            .is_empty()
    );

    let installed = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(installed.service_id, service.id);
    assert_eq!(installed.manifest.name, service.name);
    assert!(!installed.credential_set);
    assert_eq!(server.hits(), 2);
    for headers in server.headers().await {
        assert_eq!(headers["x-hito-plugin-protocol"], "1");
        assert!(headers.get("authorization").is_none());
        assert!(headers.get("cookie").is_none());
    }
}

#[tokio::test]
async fn invalid_supplied_names_are_rejected_before_manifest_lookup() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-name-validation").await;
    for name in [
        "\n".into(),
        "\t".into(),
        "\0".into(),
        "Provider\n".into(),
        " ".repeat(257),
        "é".repeat(129),
    ] {
        assert!(matches!(
            hito.plugins
                .offer_service(
                    &actor,
                    OfferPluginService {
                        name,
                        base_url: server.url.clone(),
                    },
                )
                .await,
            Err(Error::Invalid(_))
        ));
        assert_eq!(server.hits(), 0);
        assert!(
            hito.plugins
                .settings(&actor)
                .await
                .unwrap()
                .services
                .is_empty()
        );
    }

    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "   ".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(service.name, "Fixture provider");
    assert_eq!(server.hits(), 1);
}

#[tokio::test]
async fn logout_during_manifest_lookup_prevents_an_automatic_service_offer() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, admin, _library, _music) = support::library("plugin-offer-logout").await;
    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let gate = server.hold().await;
    let offering = {
        let plugins = hito.plugins.clone();
        let base_url = server.url.clone();
        tokio::spawn(async move {
            plugins
                .offer_service(
                    &actor,
                    OfferPluginService {
                        name: String::new(),
                        base_url,
                    },
                )
                .await
        })
    };
    server.entered().await;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        hito.identity.logout(&session.token),
    )
    .await
    .expect("Logout must not wait for provider HTTP to finish")
    .unwrap();
    gate.add_permits(1);

    assert!(matches!(offering.await.unwrap(), Err(Error::Unauthorized)));
    let settings = hito.plugins.settings(&admin).await.unwrap();
    assert!(settings.services.is_empty());
    assert!(settings.installations.is_empty());
    assert_eq!(server.hits(), 1);
    let headers = server.headers().await;
    assert!(headers[0].get("authorization").is_none());
    assert!(headers[0].get("cookie").is_none());
}

#[tokio::test]
async fn a_failed_automatic_lookup_saves_nothing_and_can_be_retried() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-offer-retry").await;
    let mut manifest = fixture::manifest();
    manifest["name"] = serde_json::json!("private-provider-name-sentinel\n");
    server.reply(manifest).await;
    let error = match hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: String::new(),
                base_url: server.url.clone(),
            },
        )
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("An invalid manifest must not create a service offer"),
    };
    assert!(matches!(error, Error::Invalid(_)));
    for private in ["private-provider-name-sentinel", server.url.as_str()] {
        assert!(!error.to_string().contains(private));
    }
    let settings = hito.plugins.settings(&actor).await.unwrap();
    assert!(settings.services.is_empty());
    assert!(settings.installations.is_empty());
    assert_eq!(server.hits(), 1);

    server.reply(fixture::manifest()).await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: String::new(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(service.name, "Fixture provider");
    assert_eq!(service.base_url, server.url);
    assert_eq!(server.hits(), 2);
    let settings = hito.plugins.settings(&actor).await.unwrap();
    assert_eq!(settings.services.len(), 1);
    assert_eq!(settings.services[0].id, service.id);
    assert!(settings.installations.is_empty());
}

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
                accept_manifest_digest: None,
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
                    accept_manifest_digest: None,
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
                accept_manifest_digest: None,
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
                accept_manifest_digest: None,
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

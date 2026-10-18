#[path = "fixtures/plugin_service.rs"]
mod fixture;
mod support;

use hito_core::{Error, InstallPlugin, OfferPluginService, RefreshPlugin};

#[tokio::test]
async fn withdrawing_a_service_fences_inflight_installs_and_reoffering_uses_a_new_identity() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-withdraw-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let gate = server.hold().await;
    let connecting = {
        let (plugins, actor) = (hito.plugins.clone(), actor.clone());
        let (service_id, service_revision) = (service.id.clone(), service.revision);
        tokio::spawn(async move {
            plugins
                .install(
                    &actor,
                    InstallPlugin {
                        accept_manifest_digest: None,
                        service_id,
                        service_revision,
                        bearer_token: None,
                    },
                )
                .await
        })
    };
    server.entered().await;
    assert!(matches!(
        hito.plugins
            .withdraw_service(&actor, &service.id, service.revision + 1)
            .await,
        Err(Error::Conflict(_))
    ));
    hito.plugins
        .withdraw_service(&actor, &service.id, service.revision)
        .await
        .unwrap();
    let replacement = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "New offer".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    assert_ne!(replacement.id, service.id);
    gate.add_permits(1);
    assert!(matches!(connecting.await.unwrap(), Err(Error::Conflict(_))));
    assert!(
        hito.plugins
            .settings(&actor)
            .await
            .unwrap()
            .installations
            .is_empty()
    );
}

#[tokio::test]
async fn a_delayed_refresh_cannot_restore_a_removed_personal_installation() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-remove-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
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
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    server.entered().await;
    let gate = server.hold().await;
    let refreshing = {
        let (plugins, actor) = (hito.plugins.clone(), actor.clone());
        let (id, revision) = (installed.id.clone(), installed.revision);
        tokio::spawn(async move {
            plugins
                .refresh(
                    &actor,
                    &id,
                    RefreshPlugin {
                        revision,
                        accept_manifest_digest: None,
                        credential: Default::default(),
                    },
                )
                .await
        })
    };
    server.entered().await;
    hito.plugins
        .remove(&actor, &installed.id, installed.revision)
        .await
        .unwrap();
    gate.add_permits(1);
    assert!(matches!(refreshing.await.unwrap(), Err(Error::NotFound)));
    assert!(
        hito.plugins
            .settings(&actor)
            .await
            .unwrap()
            .installations
            .is_empty()
    );
}

#[tokio::test]
async fn one_account_cannot_occupy_multiple_manifest_connections() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-admission-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let gate = server.hold().await;
    let plugins = hito.plugins.clone();
    let owner = actor.clone();
    let first_request = InstallPlugin {
        accept_manifest_digest: None,
        service_id: service.id.clone(),
        service_revision: service.revision,
        bearer_token: None,
    };
    let task = tokio::spawn(async move { plugins.install(&owner, first_request).await });
    server.entered().await;
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        hito.plugins.install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        ),
    )
    .await;
    gate.add_permits(2);
    assert!(matches!(result, Ok(Err(Error::Conflict(_)))));
    assert!(task.await.unwrap().is_ok());
    assert_eq!(server.hits(), 1);
}

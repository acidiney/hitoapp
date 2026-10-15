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
                        service_id,
                        service_revision,
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
                service_id: service.id,
                service_revision: service.revision,
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

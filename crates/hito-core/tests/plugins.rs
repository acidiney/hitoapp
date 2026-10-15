#[path = "fixtures/plugin_service.rs"]
mod fixture;
mod support;

use hito_core::{InstallPlugin, OfferPluginService};

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
            },
        )
        .await
        .unwrap();
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


use hito_core::test_support::plugin_service as fixture;
use hito_core::test_support::support;

use hito_core::{Credentials, InstallPlugin, OfferPluginService};

#[tokio::test]
async fn connections_separate_every_offered_service_from_the_ones_an_account_connected() {
    let first = fixture::PluginServer::start().await;
    let second = fixture::PluginServer::start().await;
    let (_dir, hito, admin, _library, _music) = support::library("plugin-connections").await;
    let mut offered = Vec::new();
    for (name, server) in [("Alpha service", &first), ("Beta service", &second)] {
        offered.push(
            hito.plugins
                .offer_service(
                    &admin,
                    OfferPluginService {
                        name: name.into(),
                        base_url: server.url.clone(),
                    },
                )
                .await
                .unwrap(),
        );
    }
    let alpha = &offered[0];
    let installed = hito
        .plugins
        .install(
            &admin,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: alpha.id.clone(),
                service_revision: alpha.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();

    let connections = hito.plugins.connections(&admin).await.unwrap();
    let settings = hito.plugins.settings(&admin).await.unwrap();
    assert_eq!(
        serde_json::to_value(&connections.installations).unwrap(),
        serde_json::to_value(&settings.installations).unwrap()
    );
    assert_eq!(connections.installations.len(), 1);
    assert_eq!(connections.installations[0].id, installed.id);
    let all: Vec<&str> = connections
        .offered_services()
        .map(|service| service.name.as_str())
        .collect();
    assert_eq!(all, ["Alpha service", "Beta service"]);
    let connected: Vec<&str> = connections
        .connected_services()
        .map(|service| service.name.as_str())
        .collect();
    assert_eq!(connected, ["Alpha service"]);

    hito.identity
        .create_user(
            &admin,
            Credentials {
                username: "connections-guest".into(),
                password: "owned guest connections password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "connections-guest".into(),
            password: "owned guest connections password".into(),
        })
        .await
        .unwrap();
    let guest = hito.identity.authenticate(&session.token).await.unwrap();
    let guest_connections = hito.plugins.connections(&guest).await.unwrap();
    assert!(guest_connections.installations.is_empty());
    assert_eq!(guest_connections.offered_services().count(), 2);
    assert_eq!(guest_connections.connected_services().count(), 0);
}

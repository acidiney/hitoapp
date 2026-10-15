#[path = "fixtures/plugin_service.rs"]
mod fixture;
mod support;

use hito_core::{InstallPlugin, OfferPluginService};
use serde_json::json;

#[tokio::test]
async fn invalid_manifests_never_create_personal_installations() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-protocol-host").await;
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
    let mut cases = vec![json!("not a manifest")];
    for (field, value) in [
        ("protocol_version", json!(99)),
        ("capabilities", json!(["execute_code"])),
        ("capabilities", json!([])),
        ("id", json!("../private-path")),
        ("name", json!("x".repeat(257))),
        ("name", json!("Broken\nname")),
    ] {
        let mut manifest = fixture::manifest();
        manifest[field] = value;
        cases.push(manifest);
    }
    for manifest in cases {
        server.reply(manifest).await;
        assert!(
            hito.plugins
                .install(
                    &actor,
                    InstallPlugin {
                        service_id: service.id.clone(),
                        service_revision: service.revision
                    }
                )
                .await
                .is_err()
        );
        assert!(
            hito.plugins
                .settings(&actor)
                .await
                .unwrap()
                .installations
                .is_empty()
        );
    }
    server.reply(fixture::manifest()).await;
    assert!(
        hito.plugins
            .install(
                &actor,
                InstallPlugin {
                    service_id: service.id,
                    service_revision: service.revision
                }
            )
            .await
            .is_ok()
    );
}

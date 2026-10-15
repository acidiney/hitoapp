mod support;

use axum::{Json, Router, routing::get};
use hito_core::{InstallPlugin, OfferPluginService};
use serde_json::json;

#[tokio::test]
async fn an_account_connects_an_offered_service_through_its_real_manifest_endpoint() {
    let fixture = Router::new().route("/prefix/v1/manifest", get(|| async {
        Json(json!({"id":"org.hito.test","name":"Fixture provider","protocol_version":1,"capabilities":["search","sources"]}))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/prefix/", listener.local_addr().unwrap());
    let server = tokio::spawn(async {
        axum::serve(listener, fixture).await.unwrap();
    });
    let (_dir, hito, actor, _library, _music) = support::library("plugins-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Community provider".into(),
                base_url: endpoint,
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
    server.abort();
}

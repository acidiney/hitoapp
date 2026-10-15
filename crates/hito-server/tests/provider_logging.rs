#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod fixture;
use hito_core::{
    AdoptProviderRecording, CoreConfig, Credentials, HostDeviceId, InstallPlugin,
    OfferPluginService, ProviderSearch,
};
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn provider_workflows_log_safe_outcomes_without_private_content_or_credentials() {
    let logs = Capture::default();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || captured.clone())
        .finish();
    async {
        let dir = tempfile::tempdir().unwrap();
        let hito = hito_core::open(CoreConfig::new(dir.path().join("provider-private-path.db"), dir.path().join("music"), HostDeviceId::new("provider-logs").unwrap())).await.unwrap();
        let session = hito.identity.setup(Credentials { username:"admin".into(), password:"provider-private-password".into() }).await.unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        let server = fixture::ProviderServer::start().await;
        let service = hito.plugins.offer_service(&actor, OfferPluginService { name:"Fixture".into(), base_url:server.url.clone() }).await.unwrap();
        let installed = hito.plugins.install(&actor, InstallPlugin { service_id:service.id, service_revision:1, bearer_token:Some("provider-private-token".into()) }).await.unwrap();
        let details = serde_json::json!({"id":"provider-private-remote-id","title":"provider-private-title","artist":"provider-private-artist","revision":"provider-private-revision"});
        server.reply("v1/search", serde_json::json!({"items":[details.clone()]})).await;
        server.reply("v1/recordings/detail", details).await;
        server.reply("v1/sources", serde_json::json!({"items":[{"id":"provider-private-source","label":"provider-private-label","ready":true}]})).await;
        hito.catalog.search_provider(&actor, &installed.id, ProviderSearch { installation_revision:1, query:"provider-private-query".into(), cursor:Some("provider-private-cursor".into()), limit:10 }).await.unwrap();
        let request = || AdoptProviderRecording { installation_revision:1, provider_recording_id:"provider-private-remote-id".into() };
        hito.catalog.adopt_provider(&actor, &installed.id, request()).await.unwrap();
        server.reply("v1/sources", serde_json::json!({"items":[],"unexpected":"provider-private-rejected-body"})).await;
        assert!(hito.catalog.adopt_provider(&actor, &installed.id, request()).await.is_err());
        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("Provider search completed"));
        assert!(output.contains("Provider recording adopted"));
        assert!(output.contains("Provider observation retained"));
        assert!(output.contains("upstream"));
        assert!(output.contains("account_id"));
        assert!(!output.contains("provider-private-"));
        assert!(!output.contains(&session.token));
    }.with_subscriber(subscriber).await;
}

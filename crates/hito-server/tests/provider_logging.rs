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
        let recording = hito.catalog.adopt_provider(&actor, &installed.id, request()).await.unwrap();
        let source = hito.catalog.sources(&actor,&recording.id).await.unwrap().remove(0);
        server.audio(axum::http::StatusCode::OK, axum::http::HeaderMap::from_iter([
            ("content-type".parse().unwrap(),"audio/wav".parse().unwrap())
        ]), vec![1,2,3,4]).await;
        let hito_core::OpenedAudio::Content(mut audio) = hito.catalog.open_audio(&actor,&source.id,Default::default()).await.unwrap() else { panic!("audio must open") };
        while audio.body.as_mut().unwrap().chunk().await.unwrap().is_some() {}
        drop(audio);
        server.reply("v1/sources", serde_json::json!({"items":[],"unexpected":"provider-private-rejected-body"})).await;
        assert!(hito.catalog.adopt_provider(&actor, &installed.id, request()).await.is_err());
        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("Provider search completed"));
        assert!(output.contains("Provider recording adopted"));
        assert!(output.contains("Provider observation retained"));
        assert!(output.contains("Audio transfer closed"));
        let transfer: serde_json::Value = output.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|entry| entry["fields"]["message"]=="Audio transfer closed").unwrap();
        assert!(transfer["spans"].as_array().is_some_and(|spans| spans.iter().any(|span| span["account_id"]==session.user.id)),
            "transfer completion must retain its opening account correlation");
        assert!(output.contains("upstream"));
        assert!(output.contains("account_id"));
        assert!(!output.contains("provider-private-"));
        assert!(!output.contains(&session.token));
    }.with_subscriber(subscriber).await;
}

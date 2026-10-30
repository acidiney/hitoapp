use axum::http::{HeaderMap, StatusCode};
use hito_plugin_sdk::{MANIFEST_PATH, Manifest};
use hito_testkit::fake_plugin::{FakePlugin, Gate, Reply};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub struct PluginServer {
    pub url: String,
    plugin: FakePlugin,
    awaited: AtomicUsize,
}

impl PluginServer {
    pub async fn start() -> Self {
        let manifest: Manifest = serde_json::from_value(manifest()).unwrap();
        let plugin = FakePlugin::builder(manifest).start().await;
        Self {
            url: plugin.url.clone(),
            plugin,
            awaited: AtomicUsize::new(0),
        }
    }
    pub async fn reply(&self, body: Value) {
        self.plugin.reply(MANIFEST_PATH, Reply::Json(body));
    }
    pub async fn require_bearer(&self, token: &str) {
        self.plugin.require_bearer(token);
    }
    pub async fn response(
        &self,
        status: StatusCode,
        headers: HeaderMap,
        body: Vec<u8>,
        chunked: bool,
    ) {
        self.plugin.clear_fault(MANIFEST_PATH);
        self.plugin.reply(
            MANIFEST_PATH,
            Reply::Raw {
                status,
                headers,
                body,
                chunked,
            },
        );
    }
    pub async fn hold(&self) -> Arc<Gate> {
        self.plugin.hold(MANIFEST_PATH)
    }
    pub async fn entered(&self) {
        let next = self.awaited.fetch_add(1, Ordering::SeqCst) + 1;
        self.plugin.wait_for_requests(next).await;
    }
    pub fn hits(&self) -> usize {
        self.plugin.requests().len()
    }
    pub async fn headers(&self) -> Vec<HeaderMap> {
        self.plugin
            .requests()
            .into_iter()
            .map(|request| request.headers)
            .collect()
    }
}

pub fn manifest() -> Value {
    json!({"id":"org.hito.test","name":"Fixture provider","protocol_version":1,"capabilities":["search","sources"]})
}

pub use super::observed::ObservedRequest;
use axum::http::StatusCode;
use hito_plugin_sdk::{Capability, HEALTH_PATH, HealthReport, Manifest, PROTOCOL_VERSION};
use hito_testkit::fake_plugin::{FakePlugin, Fault, Reply};
use serde_json::Value;
use std::time::Duration;

pub struct HealthServer {
    pub url: String,
    plugin: FakePlugin,
}

impl HealthServer {
    pub async fn start(name: &str, report: HealthReport) -> Self {
        let body = serde_json::to_value(report).expect("health report serializes");
        let plugin = FakePlugin::builder(Manifest {
            id: "org.hito.health-fixture".into(),
            name: name.into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Search].into(),
            authorization: None,
            content_policy: None,
        })
        .get(HEALTH_PATH, move |_| Reply::Json(body.clone()))
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            plugin,
        }
    }

    pub async fn without_health(name: &str) -> Self {
        let plugin = FakePlugin::builder(Manifest {
            id: "org.hito.legacy-health-fixture".into(),
            name: name.into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Search].into(),
            authorization: None,
            content_policy: None,
        })
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            plugin,
        }
    }

    pub fn report(&self, report: HealthReport) {
        self.reply(Reply::Json(
            serde_json::to_value(report).expect("health report serializes"),
        ));
    }

    pub fn raw(&self, value: Value) {
        self.reply(Reply::Json(value));
    }

    pub fn status(&self, status: StatusCode) {
        self.reply(Reply::Status(status));
    }

    pub fn delay(&self, delay: Duration) {
        self.plugin.clear_fault(HEALTH_PATH);
        self.plugin.fault(HEALTH_PATH, Fault::Delay(delay));
    }

    pub fn stall_body(&self) -> std::sync::Arc<hito_testkit::fake_plugin::Gate> {
        self.plugin.stall_body(HEALTH_PATH, 1)
    }

    pub fn require_bearer(&self, token: &str) {
        self.plugin.require_bearer(token);
    }

    pub fn health_requests(&self) -> Vec<ObservedRequest> {
        self.plugin
            .health_requests()
            .into_iter()
            .map(|request| ObservedRequest {
                path: format!("/prefix/{}", request.operation),
                body: request.json(),
                headers: request.headers,
                method: request.method,
            })
            .collect()
    }

    pub async fn finish(&mut self) -> bool {
        self.plugin.finish().await
    }

    fn reply(&self, reply: Reply) {
        self.plugin.reply(HEALTH_PATH, reply);
    }
}

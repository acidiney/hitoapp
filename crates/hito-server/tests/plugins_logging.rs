#[path = "../../hito-core/tests/fixtures/plugin_service.rs"]
mod fixture;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::{CoreConfig, HostDeviceId};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
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
async fn post(app: &Router, path: &str, body: Value, token: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("x-hito-request", "1");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
async fn plugin_workflow_logs_outcomes_without_tokens_names_bodies_or_input_paths() {
    let logs = Capture::default();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || captured.clone())
        .finish();
    async {
        let dir = tempfile::tempdir().unwrap();
        let services = hito_core::open(CoreConfig::new(dir.path().join("private-plugin-database-sentinel.db"), dir.path().join("music"), HostDeviceId::new("plugin-log-host").unwrap())).await.unwrap();
        let app = hito_server::router(services);
        let server = fixture::PluginServer::start().await;
        let (_, session) = post(&app, "/api/v1/setup", json!({"username":"admin","password":"plugin-log-password-sentinel"}), None).await;
        let token = session["token"].as_str().unwrap();
        let (status, service) = post(&app, "/api/v1/plugins/services", json!({"name":"private-plugin-name-sentinel","base_url":server.url}), Some(token)).await;
        assert_eq!(status, StatusCode::CREATED);
        let mut manifest = fixture::manifest();
        manifest["name"] = "private-plugin-manifest-sentinel".into();
        server.reply(manifest).await;
        let (status, installation) = post(&app, "/api/v1/plugins", json!({"service_id":service["id"],"service_revision":1,"bearer_token":"plugin-bearer-sentinel"}), Some(token)).await;
        assert_eq!(status, StatusCode::CREATED);
        let path = format!("/api/v1/plugins/{}/refresh", installation["id"].as_str().unwrap());
        assert_eq!(post(&app, &path, json!({"revision":1,"credential":{"action":"replace","bearer_token":["plugin-rejected-body-sentinel"]}}), Some(token)).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        server.response(StatusCode::BAD_GATEWAY, Default::default(), b"private-plugin-upstream-body-sentinel".to_vec(), false).await;
        assert_eq!(post(&app, &path, json!({"revision":1}), Some(token)).await.0, StatusCode::BAD_GATEWAY);
        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("Personal plugin connected"));
        assert!(output.contains("upstream_failed"));
        assert!(output.contains("request_id"));
        for secret in [token, "private-plugin-database-sentinel", "plugin-log-password-sentinel", "private-plugin-name-sentinel", "private-plugin-manifest-sentinel", "plugin-bearer-sentinel", "plugin-rejected-body-sentinel", "private-plugin-upstream-body-sentinel"] {
            assert!(!output.contains(secret), "Private value appeared in plugin logs");
        }
    }.with_subscriber(subscriber).await;
}

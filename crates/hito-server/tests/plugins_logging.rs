#[path = "../../hito-core/tests/fixtures/plugin_service.rs"]
mod fixture;
use axum::{
    Router,
    body::Body,
    extract::Query,
    http::{Request, StatusCode, Uri},
};
use hito_core::{CoreConfig, HostDeviceId};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
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

#[tokio::test]
async fn authorization_logs_outcomes_without_session_pkce_or_private_configuration_values() {
    let logs = Capture::default();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || captured.clone())
        .finish();
    async {
        let directory = tempfile::tempdir().unwrap();
        let services = hito_core::open(CoreConfig::new(
            directory.path().join("private-oauth-database-sentinel.db"),
            directory.path().join("music"),
            HostDeviceId::new("plugin-oauth-log-host").unwrap(),
        ))
        .await
        .unwrap();
        let app = hito_server::router_with_config(
            services,
            hito_server::Config {
                origin: "https://hito.test".into(),
                ..Default::default()
            },
        );
        let server = fixture::PluginServer::start().await;
        let mut manifest = fixture::manifest();
        manifest["name"] = json!("private-oauth-manifest-sentinel");
        manifest["capabilities"] = json!(["playlists"]);
        manifest["authorization"] = json!({
            "kind": "oauth2_pkce",
            "client_id": "private-oauth-client-sentinel",
            "authorization_endpoint": "https://accounts.spotify.com/authorize",
            "scopes": ["playlist-read-private"]
        });
        server.reply(manifest).await;
        let (status, session) = post(
            &app,
            "/api/v1/setup",
            json!({
                "username": "admin", "password": "private-oauth-password-sentinel"
            }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let token = session["token"].as_str().unwrap();
        let (status, service) = post(
            &app,
            "/api/v1/plugins/services",
            json!({
                "name": "private-oauth-service-sentinel", "base_url": server.url
            }),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, installation) = post(
            &app,
            "/api/v1/plugins",
            json!({
                "service_id": service["id"], "service_revision": service["revision"]
            }),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let begin_path = format!(
            "/api/v1/plugins/{}/authorization",
            installation["id"].as_str().unwrap()
        );
        let (status, attempt) = post(
            &app,
            &begin_path,
            json!({"revision": installation["revision"]}),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let authorization_url = attempt["authorization_url"].as_str().unwrap();
        let uri: Uri = authorization_url.parse().unwrap();
        let Query(query) = Query::<BTreeMap<String, String>>::try_from_uri(&uri).unwrap();
        let status_path = format!("{begin_path}/{}", attempt["id"].as_str().unwrap());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(status_path)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let pending: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(pending["status"], "pending");
        for (field, sentinel) in [
            ("code", "private-oauth-code-sentinel"),
            ("code_verifier", "private-oauth-verifier-sentinel"),
        ] {
            let mut body = json!({"revision": installation["revision"]});
            body[field] = json!(sentinel);
            assert_eq!(
                post(&app, &begin_path, body, Some(token)).await.0,
                StatusCode::UNPROCESSABLE_ENTITY
            );
        }

        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        let entries: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for message in [
            "Provider authorization started",
            "Provider authorization inspected",
        ] {
            let workflow = entries
                .iter()
                .find(|entry| entry["fields"]["message"] == message)
                .expect("Authorization must log its workflow outcome");
            assert_eq!(workflow["fields"]["outcome"], "pending");
            assert!(workflow["fields"]["elapsed_ms"].as_u64().is_some());
            assert!(
                workflow["spans"]
                    .as_array()
                    .is_some_and(|spans| spans.iter().any(|span| span["request_id"]
                        .as_str()
                        .is_some_and(|request_id| !request_id.is_empty())))
            );
        }
        for private in [
            token,
            "private-oauth-database-sentinel",
            "private-oauth-password-sentinel",
            "private-oauth-service-sentinel",
            "private-oauth-manifest-sentinel",
            "private-oauth-client-sentinel",
            "private-oauth-code-sentinel",
            "private-oauth-verifier-sentinel",
            authorization_url,
            query["state"].as_str(),
            query["code_challenge"].as_str(),
        ] {
            assert!(
                !output.contains(private),
                "Private authorization value appeared in logs"
            );
        }
    }
    .with_subscriber(subscriber)
    .await;
}

use axum::{
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

async fn assert_rejected(response: axum::response::Response, status: StatusCode, message: &str) {
    assert_eq!(response.status(), status);
    uuid::Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap()).unwrap();
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body, json!({"error": message}));
}

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
async fn requests_have_correlated_outcome_logs_without_private_data() {
    let logs = Capture::default();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || captured.clone())
        .finish();
    let dir = tempfile::tempdir().unwrap();
    let private_path = dir.path().join("private-path-sentinel.db");
    async {
        let config = CoreConfig::new(
            private_path.clone(),
            dir.path().join("music"),
            HostDeviceId::new("logging-host").unwrap(),
        );
        let services = hito_core::open(config).await.unwrap();
        let app = hito_server::router(services);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup")
                    .header("content-type", "application/json")
                    .header("x-hito-request", "1")
                    .body(Body::from(
                        json!({"username":"admin","password":"password-secret-sentinel"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let request_id = response
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        uuid::Uuid::parse_str(&request_id).unwrap();
        let session: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let token = session["token"].as_str().unwrap();
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/recordings?q=search-private-sentinel")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        for path in [
            "/api/v1/artists?q=search-private-sentinel",
            "/api/v1/releases?q=search-private-sentinel",
            "/albums?q=search-private-sentinel",
            "/artists?q=search-private-sentinel",
        ] {
            let response = app.clone().oneshot(Request::builder().uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        for path in ["/api/v1/setup", "/api/v1/login", "/api/v1/users"] {
            let response = app.clone().oneshot(Request::builder().method("POST").uri(path)
                .header("content-type", "application/json").header("x-hito-request", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(json!({"username":"admin","password":7458972365897458_u64}).to_string())).unwrap()).await.unwrap();
            assert_rejected(response, StatusCode::UNPROCESSABLE_ENTITY, "Request JSON does not match the expected fields").await;
        }
        let response = app.clone().oneshot(Request::builder().method("POST").uri("/api/v1/sync")
            .header("content-type", "application/json").header("x-hito-request", "1")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(json!({"device_id":"logging-client","operations":[{"id":uuid::Uuid::new_v4(),"action":{"kind":"invalid-sync-private-title-sentinel"}}]}).to_string())).unwrap()).await.unwrap();
        assert_rejected(response, StatusCode::UNPROCESSABLE_ENTITY, "Request JSON does not match the expected fields").await;
        for action in [
            json!({"kind":"recording_title_set","recording_id":uuid::Uuid::new_v4(),"title":"personal-title-private-sentinel"}),
            json!({"kind":"recording_title_inherit","recording_id":uuid::Uuid::new_v4()}),
            json!({"kind":"recording_artist_set","recording_id":uuid::Uuid::new_v4(),"artist":"personal-artist-private-sentinel"}),
            json!({"kind":"recording_artist_inherit","recording_id":uuid::Uuid::new_v4()}),
            json!({"kind":"release_title_set","release_id":uuid::Uuid::new_v4(),"title":"album-title-private-sentinel"}),
            json!({"kind":"release_artist_set","release_id":uuid::Uuid::new_v4(),"artist":"album-artist-private-sentinel"}),
            json!({"kind":"release_title_inherit","release_id":uuid::Uuid::new_v4()}),
            json!({"kind":"release_artist_inherit","release_id":uuid::Uuid::new_v4()}),
        ] {
            let response = app.clone().oneshot(Request::builder().method("POST").uri("/api/v1/sync")
                .header("content-type", "application/json").header("x-hito-request", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(json!({"device_id":"logging-client","operations":[{"id":uuid::Uuid::new_v4(),"action":action}]}).to_string())).unwrap()).await.unwrap();
            assert_rejected(response, StatusCode::NOT_FOUND, "Resource not found").await;
        }
        for path in [
            "/api/v1/recordings?liked=invalid-query-private-sentinel",
            "/api/v1/sync?cursor=invalid-cursor-private-sentinel",
            "/library?liked=inertia-query-private-sentinel",
        ] {
            let response = app.clone().oneshot(Request::builder().uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty()).unwrap()).await.unwrap();
            assert_rejected(response, StatusCode::BAD_REQUEST, "Request query does not match the expected fields").await;
        }
        for (content_type, body, status, message) in [
            ("application/json", "{\"username\":\"syntax-private-sentinel\",\"password\":", StatusCode::BAD_REQUEST, "Request body is not valid JSON"),
            ("text/plain", "content-type-private-sentinel", StatusCode::UNSUPPORTED_MEDIA_TYPE, "Use the application/json content type"),
        ] {
            let response = app.clone().oneshot(Request::builder().method("POST").uri("/api/v1/login")
                .header("content-type", content_type).header("x-hito-request", "1")
                .body(Body::from(body)).unwrap()).await.unwrap();
            assert_rejected(response, status, message).await;
        }
        for (body, status) in [
            (Body::from_stream(tokio_stream::iter([Err::<axum::body::Bytes, _>(std::io::Error::other("body-read-private-sentinel"))])), StatusCode::BAD_REQUEST),
            (Body::from(vec![b' '; 262145]), StatusCode::PAYLOAD_TOO_LARGE),
        ] {
            let response = app.clone().oneshot(Request::builder().method("POST").uri("/api/v1/sync")
                .header("content-type", "application/json").header("x-hito-request", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(body).unwrap()).await.unwrap();
            assert_rejected(response, status, "Request body could not be read").await;
        }
        for (method, path, body) in [
            ("GET", "/api/v1/sources/%2Fprivate-identifier-sentinel/audio", "null"),
            ("GET", "/api/v1/artists/%2Fprivate-identifier-sentinel", "null"),
            ("GET", "/api/v1/releases/%2Fprivate-identifier-sentinel", "null"),
            ("GET", "/api/v1/releases/%2Fprivate-identifier-sentinel/artwork", "null"),
            ("GET", "/api/v1/recordings/%2Fprivate-identifier-sentinel/metadata", "null"),
            ("GET", "/api/v1/releases/%2Fprivate-identifier-sentinel/metadata", "null"),
            ("GET", "/albums/%2Fprivate-identifier-sentinel/details", "null"),
            ("GET", "/recordings/%2Fprivate-identifier-sentinel", "null"),
            ("GET", "/api/v1/jobs/%2Fprivate-identifier-sentinel", "null"),
            ("GET", "/api/v1/libraries/%2Fprivate-identifier-sentinel/folders", "null"),
            ("POST", "/api/v1/libraries/%2Fprivate-identifier-sentinel/scan", "null"),
            ("POST", "/api/v1/libraries/%2Fprivate-identifier-sentinel/folders", "{\"directory\":{\"directory_id\":\"unknown\"}}"),
            ("POST", "/api/v1/folders/%2Fprivate-identifier-sentinel/scan", "null"),
            ("DELETE", "/api/v1/folders/%2Fprivate-identifier-sentinel", "{\"revision\":1}"),
        ] {
            let response = app.clone().oneshot(Request::builder().method(method).uri(path)
                .header("content-type", "application/json").header("x-hito-request", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body)).unwrap()).await.unwrap();
            assert!(response.status().is_client_error());
        }
        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(
            output.contains(&request_id),
            "missing request correlation: {output}"
        );
        assert!(
            output.contains("/api/v1/setup"),
            "missing route outcome: {output}"
        );
        assert!(output.contains("201"), "missing outcome status: {output}");
        for category in ["json_data", "json_syntax", "json_content_type", "json_body", "query"] {
            assert!(output.contains(category), "missing safe rejection category: {category}");
        }
        for secret in [
            "password-secret-sentinel",
            "search-private-sentinel",
            token,
            "private-path-sentinel",
            "7458972365897458",
            "invalid-sync-private-title-sentinel",
            "personal-title-private-sentinel",
            "personal-artist-private-sentinel",
            "album-title-private-sentinel",
            "album-artist-private-sentinel",
            "invalid-query-private-sentinel",
            "invalid-cursor-private-sentinel",
            "inertia-query-private-sentinel",
            "syntax-private-sentinel",
            "content-type-private-sentinel",
            "body-read-private-sentinel",
            "private-identifier-sentinel",
        ] {
            assert!(!output.contains(secret), "private value appeared in logs");
        }
    }
    .with_subscriber(subscriber)
    .await;
}

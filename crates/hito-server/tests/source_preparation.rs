#[allow(dead_code)]
#[path = "api/support.rs"]
mod support;

use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::{admin, app, call};

const SERVICE_TOKEN: &str = "owned-preparation-service-token";

struct SourceProvider {
    url: String,
    unexpected_requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl SourceProvider {
    async fn start() -> Self {
        let unexpected_requests = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .fallback(provider)
            .with_state(unexpected_requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            url,
            unexpected_requests,
            task,
        }
    }
}

impl Drop for SourceProvider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn provider(
    State(unexpected_requests): State<Arc<AtomicUsize>>,
    request: Request,
) -> Response {
    let response = match request.uri().path() {
        "/v1/manifest" => json!({
            "id": "org.hito.source-preparation-fixture",
            "name": "Owned source provider",
            "protocol_version": 1,
            "capabilities": ["sources"]
        }),
        "/v1/recordings/detail" => json!({
            "id": "owned-recording",
            "title": "Owned unavailable recording",
            "artist": "Owned artist"
        }),
        "/v1/sources" => json!({
            "items": [{
                "id": "owned-source",
                "label": "Owned unavailable source",
                "ready": false
            }]
        }),
        _ => {
            unexpected_requests.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    let expected_authorization = format!("Bearer {SERVICE_TOKEN}");
    if request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        != Some(expected_authorization.as_str())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(response).into_response()
}

#[tokio::test]
async fn source_preparation_requires_the_connections_approved_capability() {
    let provider = SourceProvider::start().await;
    let (_directory, app) = app().await;
    let token = admin(&app).await;
    let (status, service) = call(
        &app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Owned source provider", "base_url": provider.url}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, connection) = call(
        &app,
        "POST",
        "/api/v1/plugins",
        json!({
            "service_id": service["id"],
            "service_revision": service["revision"],
            "bearer_token": SERVICE_TOKEN
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, recording) = call(
        &app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/recordings",
            connection["id"].as_str().unwrap()
        ),
        json!({
            "installation_revision": connection["revision"],
            "provider_recording_id": "owned-recording"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recording["available"], false);
    let recording = recording["id"].as_str().unwrap();
    let (status, sources) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{recording}/sources"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert_eq!(sources[0]["ready"], false);
    let source = sources[0]["id"].as_str().unwrap();
    let (status, response) = call(
        &app,
        "POST",
        &format!("/api/v1/recordings/{recording}/sources/{source}/preparation"),
        json!({"request_id": "4e470be1-c6b6-4fbd-80f4-0258b72b1db0"}),
        Some(&token),
    )
    .await;
    assert_eq!(provider.unexpected_requests.load(Ordering::SeqCst), 0);
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

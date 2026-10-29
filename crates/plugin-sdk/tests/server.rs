use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    middleware::from_fn_with_state,
    routing::{get, post},
};
use hito_plugin_sdk::{
    Capability, Manifest, PROTOCOL_VERSION,
    server::{RequestPolicy, ServiceAuth, observe_requests, with_manifest},
};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

const TOKEN: &str = "service-token";

fn manifest() -> Manifest {
    Manifest {
        id: "org.hito.test".into(),
        name: "Test plugin".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Search].into(),
        authorization: None,
        content_policy: None,
    }
}

fn app() -> Router {
    let policy = Arc::new(RequestPolicy::new(
        hito_plugin_sdk::request_telemetry!("test"),
        Some(ServiceAuth::new(TOKEN).expect("the test token is valid")),
    ));
    with_manifest(
        Router::new()
            .route("/v1/ping", get(|| async { "pong" }))
            .route("/v1/echo", post(|body: String| async move { body })),
        manifest(),
        16,
    )
    .layer(from_fn_with_state(policy, observe_requests))
}

fn ping() -> axum::http::request::Builder {
    Request::get("/v1/ping")
}

async fn send(request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = app().oneshot(request).await.expect("the router responds");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("the body is readable")
        .to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

fn authorized() -> axum::http::request::Builder {
    ping()
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header("x-hito-plugin-protocol", "1")
}

#[tokio::test]
async fn missing_bearer_is_unauthorized() {
    let (status, _, body) = send(
        ping()
            .header("x-hito-plugin-protocol", "1")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, r#"{"error":"Plugin authorization is required"}"#);
}

#[tokio::test]
async fn duplicate_authorization_header_is_unauthorized() {
    let request = authorized()
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(request).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wrong_bearer_is_unauthorized() {
    let request = ping()
        .header(header::AUTHORIZATION, "Bearer other-token")
        .header("x-hito-plugin-protocol", "1")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(request).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn missing_protocol_header_is_invalid() {
    let request = ping()
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, r#"{"error":"The request is invalid"}"#);
}

#[tokio::test]
async fn wrong_protocol_header_is_invalid() {
    let request = ping()
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header("x-hito-plugin-protocol", "2")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(request).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn authorized_request_succeeds_with_private_headers() {
    let (status, headers, body) = send(authorized().body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "pong");
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert!(headers.get("x-request-id").is_none());
}

#[tokio::test]
async fn body_over_the_limit_is_rejected() {
    let request = Request::post("/v1/echo")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header("x-hito-plugin-protocol", "1")
        .body(Body::from("x".repeat(17)))
        .unwrap();
    assert_eq!(send(request).await.0, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn manifest_is_served_without_credentials() {
    let request = Request::get("/v1/manifest").body(Body::empty()).unwrap();
    let (status, headers, body) = send(request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    let served: Manifest = serde_json::from_str(&body).expect("the manifest is JSON");
    assert_eq!(served.id, "org.hito.test");
}

#[tokio::test]
async fn request_id_header_is_opt_in() {
    let policy = RequestPolicy::new(hito_plugin_sdk::request_telemetry!("test"), None)
        .with_request_id_header();
    let app = with_manifest(Router::new(), manifest(), 16)
        .layer(from_fn_with_state(Arc::new(policy), observe_requests));
    let response = app
        .oneshot(Request::get("/v1/manifest").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("x-request-id"));
}

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

fn authed(uri: &str) -> axum::http::request::Builder {
    Request::get(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header("x-hito-plugin-protocol", "1")
}

const INVALID_BODY: &str = r#"{"error":"The request is invalid"}"#;

#[tokio::test]
async fn query_over_4096_bytes_is_invalid() {
    let at_limit = format!("/v1/ping?q={}", "a".repeat(4094));
    assert_eq!(
        send(authed(&at_limit).body(Body::empty()).unwrap()).await.0,
        StatusCode::OK
    );
    let over = format!("/v1/ping?q={}", "a".repeat(4095));
    let (status, _, body) = send(authed(&over).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, INVALID_BODY);
}

#[tokio::test]
async fn more_than_32_headers_are_invalid() {
    let build = |extra: usize| {
        let mut request = authed("/v1/ping");
        for index in 0..extra {
            request = request.header(format!("x-extra-{index}"), "1");
        }
        request.body(Body::empty()).unwrap()
    };
    assert_eq!(send(build(30)).await.0, StatusCode::OK);
    let (status, _, body) = send(build(31)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, INVALID_BODY);
}

#[tokio::test]
async fn header_bytes_over_8192_are_invalid() {
    let under = authed("/v1/ping")
        .header("x-big", "a".repeat(1000))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(under).await.0, StatusCode::OK);
    let over = authed("/v1/ping")
        .header("x-big", "a".repeat(8192))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(over).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, INVALID_BODY);
}

#[tokio::test]
async fn limits_are_checked_before_authentication() {
    let mut request = Request::get("/v1/ping");
    for index in 0..40 {
        request = request.header(format!("x-extra-{index}"), "1");
    }
    let (status, _, body) = send(request.body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, INVALID_BODY);
    let (status, _, body) = send(ping().body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, r#"{"error":"Plugin authorization is required"}"#);
}

#[tokio::test]
async fn authentication_failures_precede_slot_and_handler_work() {
    let (status, headers, _) = send(ping().body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(headers["cache-control"], "private, no-store");
}

#[tokio::test]
async fn routes_without_layer_auth_are_served_to_anyone() {
    let policy = Arc::new(RequestPolicy::new(
        hito_plugin_sdk::request_telemetry!("test"),
        None,
    ));
    let app = with_manifest(
        Router::new().route("/v1/ping", get(|| async { "pong" })),
        manifest(),
        16,
    )
    .layer(from_fn_with_state(policy, observe_requests));
    let response = app
        .oneshot(Request::get("/v1/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

fn gated_app(entered: Arc<tokio::sync::Semaphore>, release: Arc<tokio::sync::Semaphore>) -> Router {
    let policy = Arc::new(RequestPolicy::new(
        hito_plugin_sdk::request_telemetry!("test"),
        None,
    ));
    with_manifest(
        Router::new()
            .route(
                "/v1/ping",
                get(move || {
                    let entered = entered.clone();
                    let release = release.clone();
                    async move {
                        entered.add_permits(1);
                        let _ = release.acquire().await;
                        "pong"
                    }
                }),
            )
            .route("/v1/slow", get(std::future::pending::<&'static str>)),
        manifest(),
        16,
    )
    .layer(from_fn_with_state(policy, observe_requests))
}

#[tokio::test]
async fn seventeenth_concurrent_request_is_busy() {
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let app = gated_app(entered.clone(), release.clone());
    let held: Vec<_> = (0..16)
        .map(|_| {
            let app = app.clone();
            tokio::spawn(async move {
                app.oneshot(Request::get("/v1/ping").body(Body::empty()).unwrap())
                    .await
                    .unwrap()
                    .status()
            })
        })
        .collect();
    let _ = entered.acquire_many(16).await.unwrap();
    let response = app
        .clone()
        .oneshot(Request::get("/v1/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], br#"{"error":"The service is busy"}"#);
    release.add_permits(16);
    for task in held {
        assert_eq!(task.await.unwrap(), StatusCode::OK);
    }
    let again = app
        .oneshot(Request::get("/v1/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::OK);
}

#[tokio::test(start_paused = true)]
async fn request_over_14_seconds_times_out() {
    let app = gated_app(
        Arc::new(tokio::sync::Semaphore::new(0)),
        Arc::new(tokio::sync::Semaphore::new(0)),
    );
    let started = tokio::time::Instant::now();
    let response = app
        .oneshot(Request::get("/v1/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(started.elapsed(), std::time::Duration::from_secs(14));
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        &body[..],
        br#"{"error":"The request exceeded its deadline"}"#
    );
}

#[test]
fn credential_digest_accepts_only_visible_ascii_up_to_4096_bytes() {
    use hito_plugin_sdk::server::credential_digest;
    for rejected in [
        String::new(),
        "a".repeat(4097),
        "a b".into(),
        " ".into(),
        "a\u{7f}".into(),
        "é".into(),
        "a\tb".into(),
    ] {
        assert!(credential_digest(&rejected).is_none(), "{rejected:?}");
        assert!(ServiceAuth::new(&rejected).is_none(), "{rejected:?}");
    }
    for accepted in ["a".repeat(4096), "!~".into(), "x".into()] {
        assert!(credential_digest(&accepted).is_some());
        assert!(ServiceAuth::new(&accepted).is_some());
    }
    assert_ne!(credential_digest("one"), credential_digest("two"));
}

use super::{
    playlist_fixture,
    provider_fixture::ProviderServer,
    support::{app, call},
};
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn follow_mutations_reject_invalid_json_and_browser_origins_before_changing_state() {
    let (_directory, app) = app().await;
    let (status, headers, session) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/setup")
            .header("content-type", "application/json")
            .header("x-hito-request", "1")
            .body(Body::from(
                json!({"username": "owner", "password": "owned follow input password"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let cookie = headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let token = session["token"].as_str().unwrap();
    let provider = playlist_fixture::connect(&app, token).await;
    let follow_path = format!(
        "/api/v1/plugins/{}/playlists/follow",
        provider.installation_id
    );
    let mut follow_body = json!({
        "request_id": Uuid::new_v4(),
        "installation_revision": provider.revision,
        "playlist_reference": "https://provider.invalid/playlists/input-follow",
        "interval": "hourly"
    });
    let (status, _, receipt) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri(&follow_path)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .header("x-hito-request", "1")
            .body(Body::from(follow_body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let playlist = receipt["playlist_id"].as_str().unwrap();
    let detail_path = format!("/api/v1/playlists/{playlist}");
    let status_path = format!("{detail_path}/follow");
    let refresh_path = format!("{status_path}/refresh");
    let before = saved_state(&app, token, &detail_path, &status_path).await;
    assert_eq!(before["follow"]["status"], "ready");
    assert_eq!(before["playlists"].as_array().unwrap().len(), 1);
    let observed = provider_requests(&provider.server).await;
    assert_eq!(observed, [2, 1, 0, 0, 0, 0]);
    follow_body["request_id"] = json!(Uuid::new_v4());

    for (path, valid) in [
        (follow_path.as_str(), follow_body),
        (refresh_path.as_str(), json!({"request_id": Uuid::new_v4()})),
    ] {
        let mut unknown_field = valid.clone();
        unknown_field["unexpected"] = json!("private input value");
        let mut invalid_id = valid.clone();
        invalid_id["request_id"] = json!("private-invalid-request-id");
        let mut oversized = valid.clone();
        oversized["unexpected"] = json!("x".repeat(262_144));
        let mut inputs = vec![
            (
                unknown_field.to_string(),
                Some("application/json"),
                StatusCode::UNPROCESSABLE_ENTITY,
                "Request JSON does not match the expected fields",
            ),
            (
                invalid_id.to_string(),
                Some("application/json"),
                StatusCode::UNPROCESSABLE_ENTITY,
                "Request JSON does not match the expected fields",
            ),
            (
                r#"{"request_id":"unfinished"#.to_owned(),
                Some("application/json"),
                StatusCode::BAD_REQUEST,
                "Request body is not valid JSON",
            ),
            (
                valid.to_string(),
                Some("text/plain"),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Use the application/json content type",
            ),
            (
                valid.to_string(),
                None,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Use the application/json content type",
            ),
            (
                oversized.to_string(),
                Some("application/json"),
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body could not be read",
            ),
        ];
        if valid.get("interval").is_some() {
            let mut invalid_interval = valid.clone();
            invalid_interval["interval"] = json!("private-unsupported-interval");
            inputs.push((
                invalid_interval.to_string(),
                Some("application/json"),
                StatusCode::UNPROCESSABLE_ENTITY,
                "Request JSON does not match the expected fields",
            ));
        }
        for (body, content_type, expected, message) in inputs {
            let mut request = Request::builder()
                .method("POST")
                .uri(path)
                .header("cookie", cookie)
                .header("x-hito-request", "1");
            if let Some(content_type) = content_type {
                request = request.header("content-type", content_type);
            }
            let (status, _, error) = send(&app, request.body(Body::from(body)).unwrap()).await;
            assert_eq!(status, expected);
            assert_eq!(error, json!({"error": message}));
            assert_eq!(
                saved_state(&app, token, &detail_path, &status_path).await,
                before
            );
            assert_eq!(provider_requests(&provider.server).await, observed);
        }
        for foreign_origin in [false, true] {
            let mut request = Request::builder()
                .method("POST")
                .uri(path)
                .header("cookie", cookie)
                .header("content-type", "application/json");
            if foreign_origin {
                request = request
                    .header("x-hito-request", "1")
                    .header("origin", "https://untrusted.example");
            }
            let (status, _, error) =
                send(&app, request.body(Body::from(valid.to_string())).unwrap()).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert_eq!(
                error,
                json!({"error": "Request origin could not be verified"})
            );
            assert_eq!(
                saved_state(&app, token, &detail_path, &status_path).await,
                before
            );
            assert_eq!(provider_requests(&provider.server).await, observed);
        }
    }
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}

async fn get(app: &Router, token: &str, path: &str) -> Value {
    let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn saved_state(app: &Router, token: &str, detail: &str, status: &str) -> Value {
    json!({
        "detail": get(app, token, detail).await,
        "follow": get(app, token, status).await,
        "playlists": get(app, token, "/api/v1/playlists").await,
        "catalog": get(app, token, "/api/v1/recordings").await,
        "feed": get(app, token, "/api/v1/sync?cursor=0").await,
        "activity": get(app, token, "/api/v1/activity").await
    })
}

async fn provider_requests(server: &ProviderServer) -> [usize; 6] {
    [
        server.requests("v1/playlists/detail").await.len(),
        server.requests("v1/playlists/items").await.len(),
        server.requests("v1/search").await.len(),
        server.requests("v1/recordings/detail").await.len(),
        server.requests("v1/sources").await.len(),
        server.requests("v1/audio").await.len(),
    ]
}

use hito_core::test_support::provider_audio as audio_fixture;
#[path = "api/support.rs"]
#[allow(dead_code)]
mod http;

use audio_fixture::AudioLibrary;
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use hito_core::{CoreConfig, HostDeviceId};
use http::call;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn download_mutations_reject_invalid_transport_and_browser_origins_without_changing_intents()
{
    let f = AudioLibrary::start().await;
    let app = hito_server::router(f.hito.clone());
    let (token, cookie) = login(&app).await;
    let path = format!("/api/v1/recordings/{}/download", f.source.recording_id);
    let request_id = Uuid::new_v4();
    let valid = json!({"request_id": request_id});
    let mut unknown = valid.clone();
    unknown["unexpected"] = json!("private field value");
    let oversized = json!({"request_id": request_id, "unexpected": "x".repeat(262_144)});
    for (body, media_type, expected, message) in [
        (
            unknown.to_string(),
            Some("application/json"),
            StatusCode::UNPROCESSABLE_ENTITY,
            "Request JSON does not match the expected fields",
        ),
        (
            json!({"request_id": "private-invalid-id"}).to_string(),
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
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri(&path)
            .header("cookie", &cookie)
            .header("x-hito-request", "1");
        if let Some(media_type) = media_type {
            request = request.header("content-type", media_type);
        }
        let (status, _, error) = send(&app, request.body(Body::from(body)).unwrap()).await;
        assert_eq!(status, expected);
        assert!(error == json!({"error": message}));
        assert_eq!(
            call(&app, "GET", &path, Value::Null, Some(&token)).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(f.server.requests("v1/audio").await.len(), 0);
    }

    let (status, receipt) = call(&app, "POST", &path, valid.clone(), Some(&token)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let intent = receipt["intent_id"].as_str().unwrap();
    let remove_path = format!("/api/v1/downloads/{intent}");
    let (status, before) = call(&app, "GET", &path, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(before["status"] == "queued");
    for (method, endpoint, body) in [
        ("POST", path.as_str(), valid.to_string()),
        ("DELETE", remove_path.as_str(), String::new()),
    ] {
        for foreign_origin in [false, true] {
            let mut request = Request::builder()
                .method(method)
                .uri(endpoint)
                .header("cookie", &cookie)
                .header("content-type", "application/json");
            if foreign_origin {
                request = request
                    .header("x-hito-request", "1")
                    .header("origin", "https://untrusted.example");
            }
            let (status, _, error) =
                send(&app, request.body(Body::from(body.clone())).unwrap()).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(error == json!({"error": "Request origin could not be verified"}));
            assert!(call(&app, "GET", &path, Value::Null, Some(&token)).await.1 == before);
        }
    }
    for (method, path, body) in [
        ("POST", path.as_str(), valid),
        ("GET", path.as_str(), Value::Null),
        ("DELETE", remove_path.as_str(), Value::Null),
    ] {
        assert_eq!(
            call(&app, method, path, body, None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(f.server.requests("v1/audio").await.len(), 0);
}

#[tokio::test]
async fn download_receipts_and_intent_removal_stay_scoped_to_the_account_and_backend_host() {
    let f = AudioLibrary::start().await;
    let app = hito_server::router(f.hito.clone());
    let (token, _) = login(&app).await;
    let path = format!("/api/v1/recordings/{}/download", f.source.recording_id);
    let request = json!({"request_id": Uuid::new_v4()});
    let (status, receipt) = call(&app, "POST", &path, request.clone(), Some(&token)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let intent = receipt["intent_id"].as_str().unwrap();
    let remove_path = format!("/api/v1/downloads/{intent}");
    let (status, replay) = call(&app, "POST", &path, request.clone(), Some(&token)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(replay == receipt);
    let (status, before) = call(&app, "GET", &path, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(before["status"] == "queued");
    assert!(before["host_device_id"] == "provider-audio");

    let credentials = json!({"username": "other", "password": "other download fixture password"});
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/users",
        credentials.clone(),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, other) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let other_token = other["token"].as_str().unwrap();
    assert_eq!(
        call(&app, "GET", &path, Value::Null, Some(other_token))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "DELETE", &remove_path, Value::Null, Some(other_token))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let other_host = hito_core::open(CoreConfig::new(
        f.directory.path().join("hito.db"),
        f.music.clone(),
        HostDeviceId::new("other-download-host").unwrap(),
    ))
    .await
    .unwrap();
    let other_app = hito_server::router(other_host);
    for (method, path) in [("GET", path.as_str()), ("DELETE", remove_path.as_str())] {
        assert_eq!(
            call(&other_app, method, path, Value::Null, Some(&token))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    assert!(call(&app, "GET", &path, Value::Null, Some(&token)).await.1 == before);

    for _ in 0..2 {
        assert_eq!(
            call(&app, "DELETE", &remove_path, Value::Null, Some(&token))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
    }
    let (status, replay) = call(&app, "POST", &path, request, Some(&token)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(replay == receipt);
    assert_eq!(
        call(&app, "GET", &path, Value::Null, Some(&token)).await.0,
        StatusCode::NOT_FOUND
    );
    let (status, fresh) = call(
        &app,
        "POST",
        &path,
        json!({"request_id": Uuid::new_v4()}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(fresh["intent_id"] != receipt["intent_id"]);
    let current = call(&app, "GET", &path, Value::Null, Some(&token)).await.1;
    assert_eq!(
        call(&app, "DELETE", &remove_path, Value::Null, Some(&token))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(call(&app, "GET", &path, Value::Null, Some(&token)).await.1 == current);
    assert_eq!(f.server.requests("v1/audio").await.len(), 0);
}

async fn login(app: &Router) -> (String, String) {
    let (status, headers, session) = send(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/login")
            .header("content-type", "application/json")
            .header("x-hito-request", "1")
            .body(Body::from(
                json!({"username": "admin", "password": "indexing test password"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    (
        session["token"].as_str().unwrap().to_owned(),
        headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned(),
    )
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}

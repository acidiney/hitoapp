use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use hito_core::{Credentials, test_support::provider_audio::AudioLibrary};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn response(app: &Router, token: &str, source: &str, method: &str, query: &str) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/sources/{source}/audio{query}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn remote_audio_negotiates_original_and_keeps_ordinary_passthrough() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let session = fixture
        .hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let app = hito_server::router(fixture.hito.clone());
    let query = "?quality=saver&start_ms=1000";
    let head = response(&app, &session.token, &fixture.source.id, "HEAD", query).await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()["x-hito-stream-quality"], "original");
    assert!(!head.headers().contains_key("x-hito-stream-start-ms"));
    assert!(
        head.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    let get = response(&app, &session.token, &fixture.source.id, "GET", query).await;
    assert_eq!(get.status(), StatusCode::CONFLICT);
    let original = response(&app, &session.token, &fixture.source.id, "GET", "").await;
    assert_eq!(original.status(), StatusCode::OK);
    assert_eq!(original.headers()["x-hito-stream-quality"], "original");
    assert_eq!(original.headers()[header::CONTENT_TYPE], "audio/wav");
    assert_eq!(
        original
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        fixture.original
    );
}

#[tokio::test]
async fn a_downloaded_server_managed_source_converts_without_changing_its_intent_or_original() {
    use serde_json::{Value, json};
    use uuid::Uuid;
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let session = fixture
        .hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let app = hito_server::router(fixture.hito.clone());
    let download_path = format!(
        "/api/v1/recordings/{}/download",
        fixture.source.recording_id
    );
    let created = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&download_path)
                .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"request_id": Uuid::new_v4()}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::ACCEPTED);
    assert!(
        fixture
            .hito
            .downloads
            .run_next_job_until(std::future::pending())
            .await
            .unwrap()
    );
    async fn download(app: &Router, token: &str, path: &str) -> Value {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }
    let saved = download(&app, &session.token, &download_path).await;
    assert_eq!(saved["status"], "ready");
    let source = saved["source_id"].as_str().unwrap();
    let network_reads = fixture.server.requests("v1/audio").await.len();
    let converted = response(&app, &session.token, source, "GET", "?quality=saver").await;
    assert_eq!(converted.status(), StatusCode::OK);
    assert_eq!(converted.headers()["x-hito-stream-quality"], "saver");
    assert_eq!(converted.headers()[header::CONTENT_TYPE], "audio/aac");
    let bytes = converted.into_body().collect().await.unwrap().to_bytes();
    let probed = crate::tones::probe(&bytes);
    assert_eq!(probed.codec, "aac");
    assert!((0.8..1.5).contains(&probed.seconds), "{}", probed.seconds);
    assert_eq!(download(&app, &session.token, &download_path).await, saved);
    let original = response(&app, &session.token, source, "GET", "").await;
    assert_eq!(original.status(), StatusCode::OK);
    assert_eq!(
        original
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        fixture.original
    );
    assert_eq!(
        fixture.server.requests("v1/audio").await.len(),
        network_reads
    );
}

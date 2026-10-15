use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

pub(super) async fn app() -> (tempfile::TempDir, Router) {
    let dir = tempfile::tempdir().unwrap();
    let hito = services(dir.path(), &dir.path().join("music")).await;
    (dir, hito_server::router(hito))
}

pub(super) async fn services(
    directory: &std::path::Path,
    music: &std::path::Path,
) -> hito_core::Services {
    hito_core::open(hito_core::CoreConfig::new(
        directory.join("hito.db"),
        music.to_path_buf(),
        hito_core::HostDeviceId::new("api-test-host").unwrap(),
    ))
    .await
    .unwrap()
}

pub(super) async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
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
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

pub(super) async fn admin(app: &Router) -> String {
    call(
        app,
        "POST",
        "/api/v1/setup",
        json!({"username":"admin","password":"administrator test password"}),
        None,
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .into()
}

pub(super) fn write_wav(path: &std::path::Path) -> Vec<u8> {
    let sample_count = 8000u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + sample_count * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8000u32.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(sample_count * 2).to_le_bytes());
    for i in 0..sample_count {
        bytes.extend_from_slice(&((i % 100) as i16 * 100).to_le_bytes());
    }
    std::fs::write(path, &bytes).unwrap();
    bytes
}

pub(super) async fn indexed_app() -> (tempfile::TempDir, Router, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    write_wav(&music.join("Fixture.wav"));
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let (_, libs) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{}/scan", libs[0]["id"].as_str().unwrap()),
        Value::Null,
        Some(&token),
    )
    .await;
    hito.indexer.run_next_job().await.unwrap();
    let (_, tracks) = call(&app, "GET", "/api/v1/recordings", Value::Null, Some(&token)).await;
    let recording = tracks["items"][0]["id"].as_str().unwrap().to_string();
    (dir, app, token, recording)
}

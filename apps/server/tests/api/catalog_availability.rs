use super::support::{admin, call, services};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::test_support::support::write_wav;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

async fn recordings(app: &Router, token: &str, query: &str) -> (StatusCode, Option<String>, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/recordings{query}"))
                .header("x-hito-request", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let applied = response
        .headers()
        .get("hito-recordings-availability")
        .map(|value| value.to_str().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        applied,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn recordings_acknowledge_the_applied_availability_filter() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    write_wav(&music.join("local.wav"), 4);
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
    let library = libraries[0]["id"].as_str().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{library}/scan"),
        Value::Null,
        Some(&token),
    )
    .await;
    hito.storage.run_next_job().await.unwrap();

    for (query, applied, total) in [
        ("", "all", 1),
        (
            "?availability=available_or_service",
            "available_or_service",
            1,
        ),
        ("?availability=no_source", "no_source", 0),
    ] {
        let (status, header, page) = recordings(&app, &token, query).await;
        assert_eq!(status, StatusCode::OK, "{query}");
        assert_eq!(header.as_deref(), Some(applied), "{query}");
        assert_eq!(page["total"], total, "{query}");
        assert_eq!(
            page["items"].as_array().unwrap().len(),
            total as usize,
            "{query}"
        );
    }

    let (status, header, _) = recordings(&app, &token, "?availability=ready").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(header, None);
}

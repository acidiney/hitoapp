mod support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_plugin_folder::Folder;
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn adoption_returns_authoritative_identity_and_audio_supports_head_and_single_ranges() {
    let directory = tempfile::tempdir().unwrap();
    let original = support::write_wav(
        &directory.path().join("secret.wav"),
        Some("Hito demo tone"),
        440,
    );
    let app = Folder::scan(directory.path()).unwrap().router();
    let page = support::search(&app, "demo", None, 20).await;
    let recording = &page.items[0];
    let detail = support::post(
        &app,
        "/v1/recordings/detail",
        serde_json::json!({"recording_id":recording.id}),
    )
    .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail: hito_plugin_sdk::ProviderRecording =
        serde_json::from_slice(&detail.into_body().collect().await.unwrap().to_bytes()).unwrap();
    detail.validate().unwrap();
    assert_eq!(detail.id, recording.id);
    let response = support::post(
        &app,
        "/v1/sources",
        serde_json::json!({"recording_id":recording.id}),
    )
    .await;
    let sources: hito_plugin_sdk::SourcePage =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    let uri = format!("/v1/audio?source_id={}", sources.items[0].id);
    let response = app
        .clone()
        .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    assert_eq!(response.headers()["accept-ranges"], "bytes");
    assert_eq!(
        response.headers()["content-length"],
        original.len().to_string()
    );
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        original
    );
    for (range, first, end) in [
        ("bytes=0-15", 0, 16),
        ("bytes=16-", 16, original.len()),
        ("bytes=-12", original.len() - 12, original.len()),
        ("bytes=8-9999999", 8, original.len()),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(&uri)
                    .header("range", range)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(
            response.headers()["content-range"],
            format!("bytes {first}-{}/{}", end - 1, original.len())
        );
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            original[first..end]
        );
    }
    for range in [
        "bytes=9999999-",
        "bytes=4-2",
        "bytes=0-1,5-6",
        "bytes=-0",
        "bytes=x-2",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(&uri)
                    .header("range", range)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::RANGE_NOT_SATISFIABLE,
            "{range}"
        );
        assert_eq!(
            response.headers()["content-range"],
            format!("bytes */{}", original.len())
        );
    }
    for (method, range, if_range) in [
        ("HEAD", "bytes=4-8", false),
        ("GET", "unknown=4-8", false),
        ("GET", "bytes=4-8", true),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri(&uri)
            .header("range", range);
        if if_range {
            request = request.header("if-range", "unmatched");
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-length"],
            original.len().to_string()
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        if method == "HEAD" {
            assert!(body.is_empty());
        } else {
            assert_eq!(body, original);
        }
    }
}

#[tokio::test]
async fn abandoned_audio_bodies_release_bounded_stream_capacity() {
    let directory = tempfile::tempdir().unwrap();
    support::write_wav(&directory.path().join("demo.wav"), Some("Demo"), 440);
    let app = Folder::scan(directory.path()).unwrap().router();
    let page = support::search(&app, "demo", None, 1).await;
    let uri = format!("/v1/audio?source_id=source-{}", page.items[0].id);
    let mut responses = Vec::new();
    for _ in 0..8 {
        let response = app
            .clone()
            .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        responses.push(response);
    }
    let response = app
        .clone()
        .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(responses);
    let response = app
        .clone()
        .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

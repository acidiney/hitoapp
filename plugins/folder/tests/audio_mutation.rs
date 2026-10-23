#![cfg(unix)]
mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use hito_plugin_folder::Folder;
use hito_plugin_sdk::SourcePage;
use http_body_util::BodyExt;
use std::{
    fs::{self, OpenOptions},
    io::Write,
};
use tower::ServiceExt;

async fn audio(app: &axum::Router, uri: &str, range: Option<&str>) -> Response {
    let mut request = Request::get(uri);
    if let Some(range) = range {
        request = request.header("range", range);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn admitted_audio_bodies_reject_equal_length_in_place_mutation_before_the_first_read() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let selected = music.join("private-selected.wav");
    let title = "Admitted audio binding";
    let original = support::write_wav(&selected, Some(title), 440);
    let changed = support::write_wav(
        &directory.path().join("private-replacement.wav"),
        Some(title),
        660,
    );
    assert_eq!(original.len(), changed.len());
    let state = directory.path().join("private-state");
    Folder::initialize(&music, &state).await.unwrap();
    let app = Folder::open(&music, &state).await.unwrap().router();
    let page = support::search(&app, title, None, 1).await;
    page.validate(1).unwrap();
    assert_eq!(page.items.len(), 1);
    let response = support::post(
        &app,
        "/v1/sources",
        serde_json::json!({"recording_id":page.items[0].id}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let sources: SourcePage =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    let uri = format!("/v1/audio?source_id={}", sources.items[0].id);
    let first = 65536 + 44;
    let end = first + 64;
    let range = format!("bytes={first}-{}", end - 1);
    let mut admitted = Vec::new();
    for (range, first, end) in [
        (None, 0, original.len()),
        (Some(range.as_str()), first, end),
    ] {
        assert_ne!(original[first..end], changed[first..end]);
        let expected = if range.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        };
        let response = audio(&app, &uri, range).await;
        assert_eq!(response.status(), expected);
        assert_eq!(
            response.headers()["content-length"],
            (end - first).to_string()
        );
        if range.is_some() {
            assert_eq!(
                response.headers()["content-range"],
                format!("bytes {first}-{}/{}", end - 1, original.len())
            );
        }
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            original[first..end]
        );
        let response = audio(&app, &uri, range).await;
        assert_eq!(response.status(), expected);
        admitted.push(response.into_body());
    }

    let mut writer = OpenOptions::new().write(true).open(&selected).unwrap();
    writer.write_all(&changed).unwrap();
    writer.sync_all().unwrap();
    drop(writer);
    assert_eq!(
        fs::metadata(&selected).unwrap().len(),
        original.len() as u64
    );
    for mut body in admitted {
        let mut emitted = Vec::new();
        let mut failed = false;
        while let Some(frame) = body.frame().await {
            match frame {
                Ok(frame) => {
                    if let Ok(bytes) = frame.into_data() {
                        emitted.extend_from_slice(&bytes);
                    }
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        assert!(failed, "an admitted source served a changed representation");
        assert!(emitted.is_empty(), "a changed source emitted audio bytes");
    }
}

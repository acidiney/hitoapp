use super::support::{call, indexed_app};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[path = "artwork/fixtures.rs"]
mod fixtures;

async fn image(app: &axum::Router, id: &str, token: Option<&str>) -> axum::response::Response {
    let mut request = Request::builder().uri(format!("/api/v1/recordings/{id}/artwork"));
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn artwork_uses_real_images_and_the_same_private_library_permissions() {
    let (dir, app, token, recording) = indexed_app().await;
    assert_eq!(
        image(&app, &recording, Some(&token)).await.status(),
        StatusCode::NOT_FOUND
    );
    let expected = include_bytes!("../fixtures/cover.png");
    std::fs::write(dir.path().join("music/cover.png"), expected).unwrap();
    assert_eq!(
        image(&app, &recording, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let (_, listener) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"listener","password":"artwork listener password"}),
        Some(&token),
    )
    .await;
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"listener","password":"artwork listener password"}),
        None,
    )
    .await;
    let listener_token = session["token"].as_str().unwrap();
    assert_eq!(
        image(&app, &recording, Some(listener_token)).await.status(),
        StatusCode::NOT_FOUND
    );
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
    call(
        &app,
        "PUT",
        &format!(
            "/api/v1/libraries/{}/members/{}",
            libraries[0]["id"].as_str().unwrap(),
            listener["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&token),
    )
    .await;
    let response = image(&app, &recording, Some(listener_token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        expected
    );
}

#[tokio::test]
async fn embedded_front_cover_precedes_folder_artwork() {
    use lofty::{
        picture::{MimeType, Picture, PictureType},
        prelude::TagExt,
        tag::{Tag, TagType},
    };
    let (dir, app, token, recording) = indexed_app().await;
    std::fs::write(
        dir.path().join("music/cover.png"),
        include_bytes!("../fixtures/cover.png"),
    )
    .unwrap();
    let expected = include_bytes!("../fixtures/embedded.png");
    let mut tag = Tag::new(TagType::Id3v2);
    tag.push_picture(Picture::new_unchecked(
        PictureType::CoverFront,
        Some(MimeType::Png),
        None,
        expected.to_vec(),
    ));
    tag.save_to_path(dir.path().join("music/Fixture.wav"), Default::default())
        .unwrap();
    let response = image(&app, &recording, Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        expected
    );
}

#[cfg(unix)]
#[tokio::test]
async fn folder_artwork_cannot_follow_a_symlink_outside_the_audio_folder() {
    let (dir, app, token, recording) = indexed_app().await;
    let private = dir.path().join("private.png");
    std::fs::write(&private, include_bytes!("../fixtures/cover.png")).unwrap();
    std::os::unix::fs::symlink(private, dir.path().join("music/cover.png")).unwrap();
    assert_eq!(
        image(&app, &recording, Some(&token)).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn excessive_aggregate_embedded_artwork_falls_back_to_a_safe_folder_image() {
    let (dir, app, token, recording) = fixtures::indexed_flac().await;
    fixtures::write_flac(&dir.path().join("music/Fixture.flac"), 12);
    let expected = include_bytes!("../fixtures/cover.png");
    std::fs::write(dir.path().join("music/cover.png"), expected).unwrap();
    let response = image(&app, &recording, Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.len(), expected.len());
    assert_eq!(bytes.as_ref(), expected);
}

#[tokio::test]
async fn compressed_embedded_artwork_is_skipped_for_the_safe_folder_cover() {
    let (dir, app, token, recording) = indexed_app().await;
    fixtures::add_compressed_apic(&dir.path().join("music/Fixture.wav"));
    let expected = include_bytes!("../fixtures/cover.png");
    std::fs::write(dir.path().join("music/cover.png"), expected).unwrap();
    let response = image(&app, &recording, Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.len(), expected.len());
    assert_eq!(bytes.as_ref(), expected);
}

#[cfg(unix)]
#[tokio::test]
async fn folder_artwork_skips_a_fifo_without_waiting_for_a_writer() {
    use std::{io::Write, time::Duration};

    let (dir, app, token, recording) = indexed_app().await;
    let fifo = dir.path().join("music/cover.jpg");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let response = tokio::time::timeout(
        Duration::from_secs(1),
        image(&app, &recording, Some(&token)),
    )
    .await;
    if response.is_err() {
        let mut writer = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&fifo)
            .unwrap();
        writer
            .write_all(include_bytes!("../fixtures/cover.png"))
            .unwrap();
    }
    let response = response.expect("A special file must not block an artwork request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cover_art_reads_are_private_validated_and_honest_about_unknown_covers() {
    let (_dir, app, token, _recording) = indexed_app().await;
    let mbid = "0f700000-0000-4000-8000-0000000000b1";
    for kind in ["release", "release-group"] {
        let uri = format!("/api/v1/cover-art/{kind}/{mbid}");
        let anonymous = app
            .clone()
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
        let unknown = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&uri)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    }
    for uri in [
        "/api/v1/cover-art/artist/not-an-id".to_string(),
        "/api/v1/cover-art/release/not-an-id".to_string(),
        "/api/v1/cover-art/playlist/0f700000-0000-4000-8000-0000000000b1".to_string(),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&uri)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_client_error(), "{uri}");
    }
}

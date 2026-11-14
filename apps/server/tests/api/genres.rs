use crate::support::{admin, call, services};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::test_support::{hang_guard::hang_guard, support::write_wav};
use http_body_util::BodyExt;
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use std::path::Path;
use tower::ServiceExt;

fn tagged(path: &Path, offset: i16, title: &str, genre: &str) {
    write_wav(path, offset);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(format!("Artist {title}"));
    tag.set_album(format!("Album {title}"));
    tag.set_genre(genre.into());
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

async fn discover(app: &Router, token: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/discover")
                .header("x-inertia", "true")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn genre_json_list_matches_authenticated_discover_tiles() {
    let (_directory, app, token, expected) = hang_guard(
        "owned tagged genre JSON setup",
        Box::pin(async {
            let directory = tempfile::tempdir().unwrap();
            let music = directory.path().join("music");
            std::fs::create_dir(&music).unwrap();
            tagged(&music.join("a.wav"), 1, "Song A", "Fado");
            tagged(&music.join("b.wav"), 2, "Song B", "Fado; Jazz");
            tagged(&music.join("c.wav"), 3, "Song C", "Jazz");

            let hito = services(directory.path(), &music).await;
            let app = hito_server::router(hito.clone());
            let token = admin(&app).await;
            let (status, libraries) =
                call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
            assert_eq!(status, StatusCode::OK);
            let library = libraries[0]["id"].as_str().unwrap();
            let (status, _) = call(
                &app,
                "POST",
                &format!("/api/v1/libraries/{library}/scan"),
                Value::Null,
                Some(&token),
            )
            .await;
            assert_eq!(status, StatusCode::ACCEPTED);
            assert!(hito.storage.run_next_job().await.unwrap());

            let (status, page) = discover(&app, &token).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(page["component"], "DiscoverPage");
            let genres = page["props"]["genres"].as_array().unwrap();
            assert_eq!(
                genres
                    .iter()
                    .map(|genre| (
                        genre["key"].as_str().unwrap(),
                        genre["track_count"].as_i64().unwrap()
                    ))
                    .collect::<Vec<_>>(),
                [("fado", 2), ("jazz", 2)],
                "the owned scan must produce nonempty genre tiles before the API request"
            );
            (directory, app, token, page["props"]["genres"].clone())
        }),
    )
    .await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/genres")
                .header("authorization", format!("Bearer {token}"))
                .header("x-hito-request", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let actual: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(actual, expected);
}

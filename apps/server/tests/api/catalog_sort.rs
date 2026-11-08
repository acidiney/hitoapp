use super::support::{admin, call, services};
use axum::{Router, http::StatusCode};
use hito_core::test_support::support::write_wav;
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::Value;

fn write_song(music: &std::path::Path, title: &str, album: &str, date: Option<&str>, audio: i16) {
    let file = music.join(format!("{title}.wav"));
    write_wav(&file, audio);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(format!("{title} artist"));
    tag.set_album(album.into());
    tag.insert_text(ItemKey::AlbumArtist, format!("{title} artist"));
    if let Some(date) = date {
        tag.insert_text(ItemKey::RecordingDate, date.into());
    }
    tag.save_to_path(file, WriteOptions::default()).unwrap();
}

async fn scan(app: &Router, hito: &hito_core::Services, token: &str) {
    let (_, libraries) = call(app, "GET", "/api/v1/libraries", Value::Null, Some(token)).await;
    let library = libraries[0]["id"].as_str().unwrap();
    call(
        app,
        "POST",
        &format!("/api/v1/libraries/{library}/scan"),
        Value::Null,
        Some(token),
    )
    .await;
    hito.storage.run_next_job().await.unwrap();
}

async fn titles(app: &Router, token: &str, path: &str) -> Vec<String> {
    let (status, page) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK, "{path}");
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            item["title"]
                .as_str()
                .or_else(|| item["name"].as_str())
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn the_catalog_endpoints_sort_page_and_reject_unknown_sorts() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    write_song(&music, "Bravo", "Old album", Some("1999"), 1);
    write_song(&music, "alpha", "New album", Some("2021"), 2);
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    scan(&app, &hito, &token).await;
    write_song(&music, "Charlie", "Undated album", None, 3);
    scan(&app, &hito, &token).await;

    assert_eq!(
        titles(&app, &token, "/api/v1/recordings?sort=recent&limit=1").await,
        ["Charlie"]
    );
    assert_eq!(
        titles(&app, &token, "/api/v1/recordings?sort=title").await,
        ["alpha", "Bravo", "Charlie"]
    );
    assert_eq!(
        titles(&app, &token, "/api/v1/recordings?sort=artist").await,
        ["alpha", "Bravo", "Charlie"]
    );
    assert_eq!(
        titles(&app, &token, "/api/v1/releases?sort=year").await,
        ["New album", "Old album", "Undated album"]
    );
    assert_eq!(
        titles(&app, &token, "/api/v1/releases?sort=year&limit=1&offset=1").await,
        ["Old album"]
    );
    assert_eq!(
        titles(&app, &token, "/api/v1/artists?sort=recent&limit=1").await,
        ["Charlie artist"]
    );
    let (_, albums) = call(
        &app,
        "GET",
        "/api/v1/releases?sort=year",
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(albums["items"][0]["year"], 2021);
    assert!(albums["items"][2]["year"].is_null());

    for path in [
        "/api/v1/recordings?sort=oldest",
        "/api/v1/releases?sort=",
        "/api/v1/artists?sort=Title",
        "/api/v1/recordings?sort=year",
        "/api/v1/artists?sort=year",
    ] {
        let (status, _) = call(&app, "GET", path, Value::Null, Some(&token)).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
            "{path} gave {status}"
        );
    }
}

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
        titles(&app, &token, "/api/v1/recordings?sort=album").await,
        ["alpha", "Bravo", "Charlie"]
    );
    assert_eq!(
        titles(
            &app,
            &token,
            "/api/v1/recordings?sort=album&limit=1&offset=1"
        )
        .await,
        ["Bravo"]
    );
    assert_eq!(
        titles(&app, &token, "/api/v1/recordings?sort=duration").await,
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
        "/api/v1/releases?sort=album",
        "/api/v1/releases?sort=duration",
        "/api/v1/artists?sort=album",
        "/api/v1/artists?sort=duration",
    ] {
        let (status, _) = call(&app, "GET", path, Value::Null, Some(&token)).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
            "{path} gave {status}"
        );
    }
}

fn write_tone(music: &std::path::Path, title: &str, album: Option<&str>, seconds: u32) {
    let file = music.join(format!("{title}.wav"));
    let samples = seconds * 8000;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8000u32.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(samples * 2).to_le_bytes());
    for index in 0..samples {
        bytes.extend_from_slice(&(index as i16 % 64).to_le_bytes());
    }
    std::fs::write(&file, bytes).unwrap();
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist("Owned band".into());
    if let Some(album) = album {
        tag.set_album(album.into());
        tag.insert_text(ItemKey::AlbumArtist, "Owned band".into());
    }
    tag.save_to_path(file, WriteOptions::default()).unwrap();
}

#[tokio::test]
async fn album_and_duration_sorts_order_and_page_recordings_differently_from_title() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    write_tone(&music, "alpha", Some("Zeta"), 3);
    write_tone(&music, "Bravo", Some("Alder"), 2);
    write_tone(&music, "Charlie", None, 1);
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    scan(&app, &hito, &token).await;

    assert_eq!(
        titles(&app, &token, "/api/v1/recordings?sort=title").await,
        ["alpha", "Bravo", "Charlie"]
    );
    assert_eq!(
        titles(&app, &token, "/api/v1/recordings?sort=album").await,
        ["Bravo", "alpha", "Charlie"]
    );
    assert_eq!(
        titles(
            &app,
            &token,
            "/api/v1/recordings?sort=album&limit=1&offset=1"
        )
        .await,
        ["alpha"]
    );
    assert_eq!(
        titles(&app, &token, "/api/v1/recordings?sort=duration").await,
        ["Charlie", "Bravo", "alpha"]
    );
    assert_eq!(
        titles(
            &app,
            &token,
            "/api/v1/recordings?sort=duration&limit=1&offset=1"
        )
        .await,
        ["Bravo"]
    );
}

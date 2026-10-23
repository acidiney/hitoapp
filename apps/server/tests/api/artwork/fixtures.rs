use crate::support::{admin, call, services};
use axum::Router;
use serde_json::Value;
use std::{io::Write, path::Path};

pub(super) async fn indexed_flac() -> (tempfile::TempDir, Router, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    write_flac(&music.join("Fixture.flac"), 0);
    let core = services(dir.path(), &music).await;
    let app = hito_server::router(core.clone());
    let token = admin(&app).await;
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
    call(
        &app,
        "POST",
        &format!(
            "/api/v1/libraries/{}/scan",
            libraries[0]["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&token),
    )
    .await;
    assert!(core.storage.run_next_job().await.unwrap());
    let (_, recordings) = call(&app, "GET", "/api/v1/recordings", Value::Null, Some(&token)).await;
    let recording = recordings["items"][0]["id"].as_str().unwrap().to_owned();
    (dir, app, token, recording)
}

pub(super) fn write_flac(path: &Path, picture_count: usize) {
    let input = tempfile::NamedTempFile::new().unwrap();
    crate::support::write_wav(input.path());
    assert!(
        std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(input.path())
            .args(["-c:a", "flac", "-f", "flac"])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
    if picture_count == 0 {
        return;
    }

    let mut audio = std::fs::read(path).unwrap();
    let mut offset = 4;
    loop {
        let last = audio[offset] & 0x80 != 0;
        audio[offset] &= 0x7f;
        let size = u32::from_be_bytes([0, audio[offset + 1], audio[offset + 2], audio[offset + 3]]);
        offset += 4 + size as usize;
        if last {
            break;
        }
    }
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(&audio[..offset]).unwrap();

    let mut image = include_bytes!("../../fixtures/embedded.png").to_vec();
    image.resize(2 * 1024 * 1024, 0);
    let mut picture = Vec::new();
    picture.extend_from_slice(&3u32.to_be_bytes());
    picture.extend_from_slice(&9u32.to_be_bytes());
    picture.extend_from_slice(b"image/png");
    for field in [0u32, 1, 1, 24, 0, image.len() as u32] {
        picture.extend_from_slice(&field.to_be_bytes());
    }
    picture.extend_from_slice(&image);
    let size = (picture.len() as u32).to_be_bytes();
    for index in 0..picture_count {
        let kind = if index + 1 == picture_count { 0x86 } else { 6 };
        file.write_all(&[kind, size[1], size[2], size[3]]).unwrap();
        file.write_all(&picture).unwrap();
    }
    file.write_all(&audio[offset..]).unwrap();
}

pub(super) fn add_compressed_apic(path: &Path) {
    let tag = include_bytes!("../../fixtures/compressed-apic.id3");
    let mut wav = std::fs::read(path).unwrap();
    wav.extend_from_slice(b"ID3 ");
    wav.extend_from_slice(&(tag.len() as u32).to_le_bytes());
    wav.extend_from_slice(tag);
    if !tag.len().is_multiple_of(2) {
        wav.push(0);
    }
    let size = (wav.len() as u32 - 8).to_le_bytes();
    wav[4..8].copy_from_slice(&size);
    std::fs::write(path, wav).unwrap();
}

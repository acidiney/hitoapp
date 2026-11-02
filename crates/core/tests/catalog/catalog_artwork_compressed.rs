use hito_core::test_support::{covers::COVER_A, support};

#[path = "../fixtures/id3_tags.rs"]
mod id3;

use hito_core::Search;
use id3::{Frame, Storage, Version, tag, write_mp3_with_tag};
use std::process::Command;

fn picture_payload(kind: u8, image: &[u8]) -> Vec<u8> {
    let mut payload = vec![0];
    payload.extend_from_slice(b"image/png\0");
    payload.push(kind);
    payload.extend_from_slice(b"cover\0");
    payload.extend_from_slice(image);
    payload
}

fn jpeg(scratch: &std::path::Path) -> Vec<u8> {
    let path = scratch.join("cover.jpg");
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("color=c=red:s=16x16")
        .args(["-frames:v", "1"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read(path).unwrap()
}

struct Outcome {
    artwork: Option<Vec<u8>>,
    title: String,
}

async fn scanned(frames: Vec<Frame>, version: Version) -> Outcome {
    let (dir, hito, actor, library, music) = support::library("artwork-host").await;
    write_mp3_with_tag(
        dir.path(),
        &music.join("private-file-name.mp3"),
        &tag(version, false, &frames),
    );
    support::scan(&hito, &actor, &library).await;
    let page = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    let recording = &page.items[0];
    let artwork = hito.catalog.artwork(&actor, &recording.id).await.unwrap();
    Outcome {
        artwork: artwork.map(|artwork| artwork.bytes),
        title: recording.title.clone(),
    }
}

fn title() -> Frame {
    Frame::text("TIT2", "Tagged title")
}

#[tokio::test]
async fn a_compressed_png_picture_is_extracted() {
    for version in [Version::V23, Version::V24] {
        let frames = vec![title(), Frame::zlib("APIC", picture_payload(3, COVER_A))];
        let outcome = scanned(frames, version).await;
        assert_eq!(outcome.artwork.as_deref(), Some(COVER_A));
        assert_eq!(outcome.title, "Tagged title");
    }
}

#[tokio::test]
async fn a_compressed_jpeg_picture_is_extracted_and_a_front_cover_is_preferred() {
    let scratch = tempfile::tempdir().unwrap();
    let image = jpeg(scratch.path());
    let frames = vec![
        title(),
        Frame::zlib("APIC", picture_payload(0, COVER_A)),
        Frame::zlib("APIC", picture_payload(3, &image)),
    ];
    let outcome = scanned(frames, Version::V24).await;
    assert_eq!(outcome.artwork, Some(image));
}

#[tokio::test]
async fn a_picture_over_the_size_cap_is_dropped_and_the_rest_of_the_tag_reads() {
    let mut huge = COVER_A.to_vec();
    huge.resize(9 * 1024 * 1024, 0);
    for version in [Version::V23, Version::V24] {
        let frames = vec![title(), Frame::zlib("APIC", picture_payload(3, &huge))];
        let outcome = scanned(frames, version).await;
        assert_eq!(outcome.artwork, None);
        assert_eq!(outcome.title, "Tagged title");
    }
}

#[tokio::test]
async fn a_zip_bomb_or_a_false_declared_size_is_dropped() {
    let mut bomb = COVER_A.to_vec();
    bomb.resize(600 * 1024 * 1024, 0);
    let small = picture_payload(3, COVER_A);
    let exact = small.len() as u32;
    let cases = [
        Frame::zlib("APIC", picture_payload(3, &bomb)),
        Frame {
            id: "APIC",
            payload: picture_payload(3, &bomb),
            storage: Storage::ZlibDeclaring(exact),
            unsynchronised: false,
        },
        Frame {
            id: "APIC",
            payload: small.clone(),
            storage: Storage::ZlibDeclaring(exact + 1),
            unsynchronised: false,
        },
        Frame {
            id: "APIC",
            payload: small,
            storage: Storage::ZlibDeclaring(4_000_000_000),
            unsynchronised: false,
        },
    ];
    for frame in cases {
        let outcome = scanned(vec![title(), frame], Version::V24).await;
        assert_eq!(outcome.artwork, None);
        assert_eq!(outcome.title, "Tagged title");
    }
}

#[tokio::test]
async fn corrupt_compressed_picture_data_is_dropped() {
    for version in [Version::V23, Version::V24] {
        let corrupt = Frame {
            id: "APIC",
            payload: Vec::new(),
            storage: Storage::CompressedBytes(vec![0x78, 0x9c, 1, 2, 3, 4, 5, 6, 7, 8], 80),
            unsynchronised: false,
        };
        let outcome = scanned(vec![title(), corrupt], version).await;
        assert_eq!(outcome.artwork, None);
        assert_eq!(outcome.title, "Tagged title");
    }
}

#[tokio::test]
async fn compressed_data_that_is_not_an_image_is_dropped() {
    let frames = vec![
        title(),
        Frame::zlib("APIC", picture_payload(3, b"not an image")),
    ];
    let outcome = scanned(frames, Version::V24).await;
    assert_eq!(outcome.artwork, None);
}

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
    scanned_tags(tag(version, false, &frames)).await
}

async fn scanned_tags(tags: Vec<u8>) -> Outcome {
    let (dir, hito, actor, library, music) = support::library("artwork-host").await;
    write_mp3_with_tag(dir.path(), &music.join("private-file-name.mp3"), &tags);
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

fn filler(length: usize) -> Frame {
    let mut payload = picture_payload(0, &[]);
    payload.resize(length, 0);
    Frame::zlib("APIC", payload)
}

const EIGHT_MIB: usize = 8 * 1024 * 1024;

#[tokio::test]
async fn pictures_whose_declared_sizes_pass_the_metadata_budget_are_dropped() {
    let cover = || Frame::zlib("APIC", picture_payload(3, COVER_A));
    let within = scanned(vec![title(), filler(EIGHT_MIB), cover()], Version::V24).await;
    assert_eq!(within.artwork.as_deref(), Some(COVER_A));
    let over = scanned(
        vec![title(), filler(EIGHT_MIB), filler(EIGHT_MIB), cover()],
        Version::V24,
    )
    .await;
    assert_eq!(over.artwork, None);
    assert_eq!(over.title, "Tagged title");
    let first = tag(Version::V24, false, &[title(), filler(EIGHT_MIB)]);
    let second = tag(Version::V24, false, &[filler(EIGHT_MIB)]);
    let third = tag(Version::V24, false, &[cover()]);
    let stacked = scanned_tags([first, second, third].concat()).await;
    assert_eq!(stacked.artwork, None);
}

#[tokio::test]
async fn real_bombs_with_small_or_near_cap_declared_sizes_are_dropped() {
    let mut bomb = COVER_A.to_vec();
    bomb.resize(64 * 1024 * 1024, 0);
    let near_cap = (EIGHT_MIB + 60 * 1024) as u32;
    for version in [Version::V23, Version::V24] {
        for declared in [picture_payload(3, COVER_A).len() as u32, near_cap] {
            let frame = Frame {
                id: "APIC",
                payload: picture_payload(3, &bomb),
                storage: Storage::ZlibDeclaring(declared),
                unsynchronised: false,
            };
            let outcome = scanned(vec![title(), frame], version).await;
            assert_eq!(outcome.artwork, None, "{declared}");
            assert_eq!(outcome.title, "Tagged title");
        }
    }
}

#[tokio::test]
async fn an_honest_picture_just_under_the_inflate_cap_is_still_not_an_image_over_8_mib() {
    let mut image = COVER_A.to_vec();
    image.resize(EIGHT_MIB + 1024, 0);
    let outcome = scanned(
        vec![title(), Frame::zlib("APIC", picture_payload(3, &image))],
        Version::V24,
    )
    .await;
    assert_eq!(outcome.artwork, None);
}

#[tokio::test]
async fn only_the_first_four_stacked_tags_are_searched_for_pictures() {
    let plain = |text: &str| tag(Version::V24, false, &[Frame::text("TIT2", text)]);
    let picture = tag(
        Version::V24,
        false,
        &[Frame::zlib("APIC", picture_payload(3, COVER_A))],
    );
    let fourth = scanned_tags([plain("a"), plain("b"), plain("c"), picture.clone()].concat()).await;
    assert_eq!(fourth.artwork.as_deref(), Some(COVER_A));
    let fifth =
        scanned_tags([plain("a"), plain("b"), plain("c"), plain("d"), picture].concat()).await;
    assert_eq!(fifth.artwork, None);
}

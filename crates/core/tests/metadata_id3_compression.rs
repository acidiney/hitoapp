use hito_core::test_support::support;

#[path = "fixtures/id3_tags.rs"]
mod id3;

use hito_core::{Action, MetadataHistoryQuery, Search};
use id3::{
    Frame, Layout, Storage, Version, append_aiff_id3_chunk, append_wav_id3_chunk, tag, tag_with,
    utf16_payload, write_mp3_with_tag,
};
use std::path::Path;

#[derive(Debug, PartialEq)]
struct Observed {
    title: String,
    artist: String,
    album: String,
    title_from_tag: bool,
    artist_from_tag: bool,
}

enum Carrier {
    Mp3,
    Wav,
    Aiff,
}

const FILE_NAME: &str = "private-file-name";

fn place(carrier: &Carrier, scratch: &Path, music: &Path, tag: &[u8]) {
    match carrier {
        Carrier::Mp3 => write_mp3_with_tag(scratch, &music.join(format!("{FILE_NAME}.mp3")), tag),
        Carrier::Wav => {
            let path = music.join(format!("{FILE_NAME}.wav"));
            support::write_wav(&path, 0);
            append_wav_id3_chunk(&path, tag);
        }
        Carrier::Aiff => {
            let source = scratch.join("aiff-source.wav");
            support::write_wav(&source, 0);
            let path = music.join(format!("{FILE_NAME}.aiff"));
            let status = std::process::Command::new("ffmpeg")
                .args(["-nostdin", "-v", "error", "-y", "-i"])
                .arg(&source)
                .args(["-c:a", "pcm_s16be"])
                .arg(&path)
                .status()
                .unwrap();
            assert!(status.success());
            append_aiff_id3_chunk(&path, tag);
        }
    }
}

async fn scanned(carrier: Carrier, tag: Vec<u8>) -> Observed {
    let (dir, hito, actor, library, music) = support::library("id3-host").await;
    place(&carrier, dir.path(), &music, &tag);
    support::scan(&hito, &actor, &library).await;
    let page = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1, "the file must stay in the library");
    let recording = &page.items[0];
    let detail = hito
        .catalog
        .recording_metadata(&actor, &recording.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    let tags = detail.selected_observation.unwrap().tags.unwrap();
    Observed {
        title: recording.title.clone(),
        artist: recording.artist.clone(),
        album: recording.album.clone().unwrap(),
        title_from_tag: tags.title.is_some(),
        artist_from_tag: tags.artist.is_some(),
    }
}

fn compressed_set() -> Vec<Frame> {
    vec![
        Frame::compressed_text("TIT2", "Compressed title"),
        Frame::compressed_text("TPE1", "Compressed artist \u{c9}toile"),
        Frame::compressed_text("TALB", "Compressed album"),
    ]
}

fn carriers() -> [Carrier; 2] {
    [Carrier::Mp3, Carrier::Wav]
}

fn compressed_expectation() -> Observed {
    Observed {
        title: "Compressed title".into(),
        artist: "Compressed artist \u{c9}toile".into(),
        album: "Compressed album".into(),
        title_from_tag: true,
        artist_from_tag: true,
    }
}

fn plain_artist_with_filename_title() -> Observed {
    Observed {
        title: FILE_NAME.into(),
        artist: "Plain artist".into(),
        album: "Unknown album".into(),
        title_from_tag: false,
        artist_from_tag: true,
    }
}

#[tokio::test]
async fn compressed_text_frames_are_read_from_id3v24_tags() {
    for carrier in carriers() {
        let tag = tag(Version::V24, false, &compressed_set());
        assert_eq!(scanned(carrier, tag).await, compressed_expectation());
    }
}

#[tokio::test]
async fn compressed_text_frames_are_read_from_id3v23_tags() {
    for carrier in carriers() {
        let tag = tag(Version::V23, false, &compressed_set());
        assert_eq!(scanned(carrier, tag).await, compressed_expectation());
    }
}

#[tokio::test]
async fn compression_works_with_unsynchronisation() {
    let mut v24 = compressed_set();
    for frame in &mut v24 {
        frame.unsynchronised = true;
    }
    let observed = scanned(Carrier::Mp3, tag(Version::V24, false, &v24)).await;
    assert_eq!(observed, compressed_expectation());
    let observed = scanned(Carrier::Mp3, tag(Version::V23, true, &compressed_set())).await;
    assert_eq!(observed, compressed_expectation());
}

fn corrupt_title() -> Vec<Frame> {
    vec![
        Frame {
            id: "TIT2",
            payload: Vec::new(),
            storage: Storage::CompressedBytes(vec![0x78, 0x9c, 1, 2, 3, 4, 5, 6, 7, 8], 40),
            unsynchronised: false,
        },
        Frame::text("TPE1", "Plain artist"),
    ]
}

#[tokio::test]
async fn a_corrupt_compressed_frame_is_ignored_and_other_frames_are_kept() {
    for version in [Version::V23, Version::V24] {
        for carrier in carriers() {
            let tag = tag(version, false, &corrupt_title());
            assert_eq!(
                scanned(carrier, tag).await,
                plain_artist_with_filename_title()
            );
        }
    }
}

fn repeated_title(length: usize, storage: Storage) -> Vec<Frame> {
    let mut payload = vec![3];
    payload.extend(std::iter::repeat_n(b'a', length));
    vec![
        Frame {
            id: "TIT2",
            payload,
            storage,
            unsynchronised: false,
        },
        Frame::text("TPE1", "Plain artist"),
    ]
}

#[tokio::test]
async fn frames_over_the_size_or_expansion_limits_are_ignored() {
    for version in [Version::V23, Version::V24] {
        for length in [200 * 1024, 4 * 1024 * 1024, 64 * 1024 * 1024] {
            let frames = repeated_title(length, Storage::Zlib);
            let observed = scanned(Carrier::Mp3, tag(version, false, &frames)).await;
            assert_eq!(observed, plain_artist_with_filename_title(), "{length}");
        }
    }
}

#[tokio::test]
async fn a_declared_size_that_lies_is_ignored() {
    for version in [Version::V23, Version::V24] {
        for declared in [0, 3, 6, 4_000_000_000_u32] {
            let frames = repeated_title(4, Storage::ZlibDeclaring(declared));
            let observed = scanned(Carrier::Mp3, tag(version, false, &frames)).await;
            assert_eq!(observed, plain_artist_with_filename_title(), "{declared}");
        }
    }
}

#[tokio::test]
async fn a_compressed_frame_the_library_does_not_read_is_ignored_without_hiding_the_file() {
    for version in [Version::V23, Version::V24] {
        let frames = vec![
            Frame::zlib("COMM", vec![0, b'e', b'n', b'g', 0, b'h', b'i']),
            Frame::compressed_text("TIT2", "Compressed title"),
            Frame::text("TPE1", "Plain artist"),
        ];
        let observed = scanned(Carrier::Wav, tag(version, false, &frames)).await;
        assert_eq!(observed.title, "Compressed title");
        assert_eq!(observed.artist, "Plain artist");
    }
}

#[tokio::test]
async fn uncompressed_tags_are_read_as_before() {
    for version in [Version::V23, Version::V24] {
        for carrier in carriers() {
            let frames = vec![
                Frame::text("TIT2", "Plain title"),
                Frame::text("TPE1", "Plain artist"),
                Frame::text("TALB", "Plain album"),
            ];
            let observed = scanned(carrier, tag(version, false, &frames)).await;
            assert_eq!(observed.title, "Plain title");
            assert_eq!(observed.album, "Plain album");
            assert!(observed.title_from_tag);
        }
    }
}

#[tokio::test]
async fn an_ignored_frame_keeps_filename_provenance_and_personal_edits() {
    let (dir, hito, actor, library, music) = support::library("id3-host").await;
    place(
        &Carrier::Mp3,
        dir.path(),
        &music,
        &tag(Version::V24, false, &corrupt_title()),
    );
    support::scan(&hito, &actor, &library).await;
    let recording = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    support::synchronize(
        &hito,
        &actor,
        vec![Action::RecordingTitleSet {
            recording_id: recording.id.parse().unwrap(),
            title: "Personal title".into(),
        }],
    )
    .await;
    place(
        &Carrier::Mp3,
        dir.path(),
        &music,
        &tag(Version::V24, false, &compressed_set()),
    );
    support::scan(&hito, &actor, &library).await;
    let detail = hito
        .catalog
        .recording_metadata(&actor, &recording.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(detail.title.inherited, "Compressed title");
    assert_eq!(detail.title.value, "Personal title");
}

#[tokio::test]
async fn compressed_frames_are_read_from_an_aiff_id3_chunk() {
    for version in [Version::V23, Version::V24] {
        let observed = scanned(Carrier::Aiff, tag(version, false, &compressed_set())).await;
        assert_eq!(observed, compressed_expectation());
    }
}

#[tokio::test]
async fn extended_headers_are_removed_around_compressed_frames() {
    for version in [Version::V23, Version::V24] {
        let layout = Layout {
            extended_header: true,
            ..Layout::default()
        };
        let observed = scanned(Carrier::Mp3, tag_with(version, layout, &compressed_set())).await;
        assert_eq!(observed, compressed_expectation());
    }
}

fn mixed_set() -> Vec<Frame> {
    vec![
        Frame::compressed_text("TIT2", "Compressed title"),
        Frame::plain("TPE1", utf16_payload("Plain artist \u{c9}toile")),
        Frame::compressed_text("TALB", "Compressed album"),
    ]
}

#[tokio::test]
async fn id3v24_tag_level_unsynchronisation_applies_to_every_frame() {
    let layout = Layout {
        unsynchronised: true,
        ..Layout::default()
    };
    for carrier in carriers() {
        let observed = scanned(carrier, tag_with(Version::V24, layout, &mixed_set())).await;
        assert_eq!(observed.title, "Compressed title");
        assert_eq!(observed.artist, "Plain artist \u{c9}toile");
        assert_eq!(observed.album, "Compressed album");
    }
}

#[tokio::test]
async fn a_footer_does_not_stop_the_walk_over_stacked_tags() {
    let layout = Layout {
        footer: true,
        ..Layout::default()
    };
    let mut bytes = tag_with(
        Version::V24,
        layout,
        &[Frame::compressed_text("TIT2", "Compressed title")],
    );
    bytes.extend(tag(
        Version::V24,
        false,
        &[Frame::compressed_text("TALB", "Compressed album")],
    ));
    let observed = scanned(Carrier::Mp3, bytes).await;
    assert_eq!(observed.title, "Compressed title");
    assert_eq!(observed.album, "Compressed album");
}

#[tokio::test]
async fn many_expensive_failing_frames_stay_bounded_and_the_file_stays_indexed() {
    let mut frames = vec![Frame::text("TPE1", "Plain artist")];
    for _ in 0..200 {
        frames.push(Frame {
            id: "TIT2",
            payload: Vec::new(),
            storage: Storage::CompressedBytes([0x78, 0x9c, 7].repeat(6000), 1024 * 1024),
            unsynchronised: false,
        });
    }
    let observed = scanned(Carrier::Mp3, tag(Version::V24, false, &frames)).await;
    assert_eq!(observed, plain_artist_with_filename_title());
}

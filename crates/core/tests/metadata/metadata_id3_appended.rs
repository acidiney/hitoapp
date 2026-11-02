use hito_core::test_support::support;

#[path = "../fixtures/id3_tags.rs"]
mod id3;

use hito_core::{MetadataHistoryQuery, Search};
use id3::{Frame, Layout, Version, id3v1, tag_with, write_bare_mp3_around, write_mp3_around};

const FILE_NAME: &str = "private-file-name";

#[derive(Debug, PartialEq)]
struct Observed {
    title: String,
    artist: String,
    album: String,
    title_from_tag: bool,
}

async fn scanned_file(bare: bool, leading: &[u8], trailing: &[u8]) -> Option<Observed> {
    let (dir, hito, actor, library, music) = support::library("id3-host").await;
    let write = if bare {
        write_bare_mp3_around
    } else {
        write_mp3_around
    };
    write(
        dir.path(),
        &music.join(format!("{FILE_NAME}.mp3")),
        leading,
        trailing,
    );
    support::scan(&hito, &actor, &library).await;
    let page = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    if page.total == 0 {
        return None;
    }
    assert_eq!(page.total, 1);
    let recording = &page.items[0];
    let detail = hito
        .catalog
        .recording_metadata(&actor, &recording.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    let tags = detail.selected_observation.unwrap().tags.unwrap();
    Some(Observed {
        title: recording.title.clone(),
        artist: recording.artist.clone(),
        album: recording.album.clone().unwrap(),
        title_from_tag: tags.title.is_some(),
    })
}

async fn scanned(leading: &[u8], trailing: &[u8]) -> Observed {
    scanned_file(false, leading, trailing)
        .await
        .expect("the file must stay in the library")
}

fn footer() -> Layout {
    Layout {
        footer: true,
        ..Layout::default()
    }
}

fn appended(frames: &[Frame]) -> Vec<u8> {
    tag_with(Version::V24, footer(), frames)
}

fn leading(frames: &[Frame]) -> Vec<u8> {
    tag_with(Version::V24, Layout::default(), frames)
}

fn appended_set() -> Vec<Frame> {
    vec![
        Frame::text("TIT2", "Appended title"),
        Frame::text("TPE1", "Appended artist"),
        Frame::text("TALB", "Appended album"),
    ]
}

fn appended_expectation() -> Observed {
    Observed {
        title: "Appended title".into(),
        artist: "Appended artist".into(),
        album: "Appended album".into(),
        title_from_tag: true,
    }
}

fn filename_only() -> Observed {
    Observed {
        title: FILE_NAME.into(),
        artist: "Unknown artist".into(),
        album: "Unknown album".into(),
        title_from_tag: false,
    }
}

#[tokio::test]
async fn an_appended_id3v24_tag_is_read_with_or_without_an_encoder_only_leading_tag() {
    let tag = appended(&appended_set());
    assert_eq!(scanned(&[], &tag).await, appended_expectation());
    let bare = scanned_file(true, &[], &tag).await;
    assert_eq!(bare, Some(appended_expectation()));
}

#[tokio::test]
async fn compressed_frames_in_an_appended_tag_are_read_and_bad_ones_dropped() {
    let frames = vec![
        Frame::compressed_text("TIT2", "Appended title"),
        Frame {
            id: "TALB",
            payload: Vec::new(),
            storage: id3::Storage::CompressedBytes(vec![0x78, 0x9c, 1, 2, 3, 4], 40),
            unsynchronised: false,
        },
        Frame::text("TPE1", "Appended artist"),
    ];
    let observed = scanned(&[], &appended(&frames)).await;
    assert_eq!(observed.title, "Appended title");
    assert_eq!(observed.artist, "Appended artist");
    assert_eq!(observed.album, "Unknown album");
}

#[tokio::test]
async fn a_leading_tag_wins_and_the_appended_tag_only_fills_missing_fields() {
    let head = leading(&[
        Frame::text("TIT2", "Leading title"),
        Frame::text("TPE1", "Leading artist"),
    ]);
    let observed = scanned(&head, &appended(&appended_set())).await;
    assert_eq!(
        observed,
        Observed {
            title: "Leading title".into(),
            artist: "Leading artist".into(),
            album: "Appended album".into(),
            title_from_tag: true,
        }
    );
}

#[tokio::test]
async fn a_footer_that_does_not_match_a_tag_header_is_not_read() {
    let mut size_lies = appended(&appended_set());
    let length = size_lies.len();
    size_lies[length - 4..].copy_from_slice(&[0, 0, 0x7f, 0x7f]);
    let mut no_header = appended(&appended_set());
    no_header[0] = b'X';
    let mut not_syncsafe = appended(&appended_set());
    let length = not_syncsafe.len();
    not_syncsafe[length - 7] = 0x80;
    for tag in [size_lies, no_header, not_syncsafe] {
        assert_eq!(scanned_file(false, &[], &tag).await, None);
    }
}

#[tokio::test]
async fn an_appended_tag_over_the_size_limit_is_dropped_and_the_file_stays() {
    let mut frames = appended_set();
    frames.push(Frame::plain("PRIV", vec![0; 16 * 1024 * 1024 + 1]));
    assert_eq!(scanned(&[], &appended(&frames)).await, filename_only());
}

#[tokio::test]
async fn an_appended_tag_before_an_id3v1_tag_is_read_and_wins_over_it() {
    let trailer = id3v1("Version one title", "Version one artist");
    let observed = scanned(&[], &[appended(&appended_set()), trailer].concat()).await;
    assert_eq!(observed, appended_expectation());
}

use hito_core::test_support::support;

#[path = "../fixtures/id3_tags.rs"]
mod id3;

use hito_core::{MetadataHistoryQuery, Search};
use id3::{
    Frame, Layout, Version, append_wav_id3_chunk, tag_with, text_payload, write_mp3_with_tag,
};

const RECORDING: &str = "a1000000-0000-4000-8000-000000000003";
const OTHER: &str = "a1000000-0000-4000-8000-000000000009";
const OWNER: &str = "http://musicbrainz.org";

fn ufid(owner: &str, identifier: &[u8]) -> Frame {
    let mut payload = owner.as_bytes().to_vec();
    payload.push(0);
    payload.extend(identifier);
    Frame::plain("UFID", payload)
}

fn txxx(description: &str, value: &str) -> Frame {
    let mut payload = text_payload(description);
    payload.push(0);
    payload.extend(value.as_bytes());
    Frame::plain("TXXX", payload)
}

async fn recording_id(
    wav: bool,
    version: Version,
    layout: Layout,
    frames: Vec<Frame>,
) -> Option<String> {
    let (dir, hito, actor, library, music) = support::library("ufid-host").await;
    let tag = tag_with(version, layout, &frames);
    if wav {
        let path = music.join("private-file-name.wav");
        support::write_wav(&path, 0);
        append_wav_id3_chunk(&path, &tag);
    } else {
        write_mp3_with_tag(dir.path(), &music.join("private-file-name.mp3"), &tag);
    }
    support::scan(&hito, &actor, &library).await;
    let page = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    let detail = hito
        .catalog
        .recording_metadata(&actor, &page.items[0].id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    let observation = detail.selected_observation.unwrap();
    assert_eq!(observation.inspection_version, Some(9));
    observation
        .tags
        .unwrap()
        .musicbrainz_recording_id
        .map(|tag| tag.value)
}

fn layout(unsynchronised: bool, extended_header: bool, footer: bool) -> Layout {
    Layout {
        unsynchronised,
        extended_header,
        footer,
    }
}

#[tokio::test]
async fn the_ufid_recording_id_is_read_in_every_id3_layout_and_carrier() {
    let mut failures = Vec::new();
    for wav in [false, true] {
        for (version, name, layouts) in [
            (
                Version::V23,
                "v2.3",
                vec![
                    layout(false, false, false),
                    layout(true, false, false),
                    layout(false, true, false),
                ],
            ),
            (
                Version::V24,
                "v2.4",
                vec![
                    layout(false, false, false),
                    layout(true, false, false),
                    layout(false, true, false),
                    layout(false, false, true),
                ],
            ),
        ] {
            for (index, layout) in layouts.into_iter().enumerate() {
                let frames = vec![
                    Frame::text("TIT2", "Title"),
                    ufid(OWNER, RECORDING.as_bytes()),
                ];
                let found = recording_id(wav, version, layout, frames).await;
                if found.as_deref() != Some(RECORDING) {
                    failures.push(format!("wav={wav} {name} layout {index}: {found:?}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

type Case = (&'static str, Vec<Frame>, Option<&'static str>);

fn cases() -> Vec<Case> {
    let long = vec![b'a'; 200];
    vec![
        (
            "other owner",
            vec![ufid("http://example.org", RECORDING.as_bytes())],
            None,
        ),
        ("oversized identifier", vec![ufid(OWNER, &long)], None),
        (
            "compressed frame",
            vec![Frame::zlib(
                "UFID",
                [OWNER.as_bytes(), b"\0", RECORDING.as_bytes()].concat(),
            )],
            None,
        ),
        (
            "ambiguous text frame only",
            vec![txxx("MusicBrainz Track Id", OTHER)],
            None,
        ),
        (
            "frame wins over text frame",
            vec![
                txxx("MusicBrainz Track Id", OTHER),
                ufid(OWNER, RECORDING.as_bytes()),
            ],
            Some(RECORDING),
        ),
        (
            "malformed value is kept as a raw assertion",
            vec![ufid(OWNER, b"not-a-recording-id")],
            Some("not-a-recording-id"),
        ),
        (
            "compressed text neighbours do not hide it",
            vec![
                Frame::compressed_text("TIT2", "Title"),
                ufid(OWNER, RECORDING.as_bytes()),
            ],
            Some(RECORDING),
        ),
    ]
}

#[tokio::test]
async fn unusable_ufid_frames_leave_the_recording_id_absent_and_never_use_the_ambiguous_text_frame()
{
    let mut failures = Vec::new();
    for wav in [false, true] {
        for (name, frames, expected) in cases() {
            let found = recording_id(wav, Version::V24, Layout::default(), frames).await;
            if found.as_deref() != expected {
                failures.push(format!("wav={wav} {name}: {found:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

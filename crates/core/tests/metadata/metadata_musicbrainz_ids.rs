use hito_core::test_support::support;
use hito_core::{MetadataHistoryQuery, Search};
use lofty::{
    TextEncoding,
    config::WriteOptions,
    id3::v2::{ExtendedTextFrame, Frame, Id3v2Tag, UniqueFileIdentifierFrame},
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::process::Command;

const RELEASE: &str = "a1000000-0000-4000-8000-000000000001";
const GROUP: &str = "a1000000-0000-4000-8000-000000000002";
const RECORDING: &str = "a1000000-0000-4000-8000-000000000003";
const RELEASE_TRACK: &str = "a1000000-0000-4000-8000-000000000004";

struct Case {
    name: &'static str,
    extension: &'static str,
    encoder: &'static [&'static str],
    tag_type: TagType,
}

const CASES: [Case; 5] = [
    Case {
        name: "flac vorbis comments",
        extension: "flac",
        encoder: &["-c:a", "flac"],
        tag_type: TagType::VorbisComments,
    },
    Case {
        name: "ogg vorbis comments",
        extension: "ogg",
        encoder: &["-c:a", "libvorbis"],
        tag_type: TagType::VorbisComments,
    },
    Case {
        name: "mp3 id3v2 TXXX and UFID",
        extension: "mp3",
        encoder: &[
            "-c:a",
            "libmp3lame",
            "-write_id3v2",
            "0",
            "-write_xing",
            "0",
        ],
        tag_type: TagType::Id3v2,
    },
    Case {
        name: "m4a freeform atoms",
        extension: "m4a",
        encoder: &["-c:a", "aac"],
        tag_type: TagType::Mp4Ilst,
    },
    Case {
        name: "wav id3v2 chunk TXXX and UFID",
        extension: "wav",
        encoder: &["-c:a", "pcm_s16le"],
        tag_type: TagType::Id3v2,
    },
];

async fn observed(case: &Case, with_ids: bool) -> Option<[Option<String>; 4]> {
    let (dir, hito, actor, library, music) = support::library("musicbrainz-id-tag-host").await;
    let source = dir.path().join("source.wav");
    support::write_wav(&source, 0);
    let path = music.join(format!("tagged.{}", case.extension));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(&source)
        .args(case.encoder)
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let mut tag = Tag::new(case.tag_type);
    tag.set_title("Tagged recording".into());
    tag.set_artist("Tagged credit".into());
    if with_ids {
        for (key, value) in [
            (ItemKey::MusicBrainzReleaseId, RELEASE),
            (ItemKey::MusicBrainzReleaseGroupId, GROUP),
            (ItemKey::MusicBrainzRecordingId, RECORDING),
            (ItemKey::MusicBrainzTrackId, RELEASE_TRACK),
        ] {
            if case.tag_type == TagType::Id3v2 && key == ItemKey::MusicBrainzRecordingId {
                continue;
            }
            assert!(tag.insert_text(key, value.into()), "{}", case.name);
        }
    }
    if case.tag_type == TagType::Id3v2 {
        let mut id3 = Id3v2Tag::from(tag);
        if with_ids {
            id3.insert(Frame::UserText(ExtendedTextFrame::new(
                TextEncoding::UTF8,
                "MusicBrainz Track Id".into(),
                RECORDING.into(),
            )));
            id3.insert(Frame::UniqueFileIdentifier(UniqueFileIdentifierFrame::new(
                "http://musicbrainz.org".into(),
                RECORDING.as_bytes().to_vec(),
            )));
        }
        id3.save_to_path(&path, WriteOptions::default()).unwrap();
    } else {
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
    }
    support::scan(&hito, &actor, &library).await;
    let page = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1, "{}", case.name);
    let detail = hito
        .catalog
        .recording_metadata(&actor, &page.items[0].id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    let tags = detail.selected_observation.unwrap().tags.unwrap();
    let value = |tag: Option<hito_core::ObservedTag>| tag.map(|tag| tag.value);
    Some([
        value(tags.musicbrainz_album_id),
        value(tags.musicbrainz_release_group_id),
        value(tags.musicbrainz_recording_id),
        value(tags.musicbrainz_release_track_id),
    ])
}

#[tokio::test]
async fn release_and_recording_musicbrainz_ids_are_retained_as_source_assertions_in_every_supported_dialect()
 {
    let all = [RELEASE, GROUP, RECORDING, RELEASE_TRACK].map(|id| Some(id.to_owned()));
    let mut failures = Vec::new();
    for case in &CASES {
        let found = observed(case, true).await;
        if found.as_ref() != Some(&all) {
            failures.push(format!("{}: {found:?}", case.name));
        }
        assert_eq!(
            observed(case, false).await,
            Some([None, None, None, None]),
            "{}",
            case.name
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

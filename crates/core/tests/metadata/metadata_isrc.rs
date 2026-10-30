use hito_core::test_support::support;
use hito_core::{MetadataHistoryQuery, Search};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::process::Command;

const ISRC: &str = "usrc17607839";

struct Case {
    name: &'static str,
    extension: &'static str,
    encoder: &'static [&'static str],
    tag_type: TagType,
}

const CASES: [Case; 5] = [
    Case {
        name: "flac vorbis ISRC",
        extension: "flac",
        encoder: &["-c:a", "flac"],
        tag_type: TagType::VorbisComments,
    },
    Case {
        name: "ogg vorbis ISRC",
        extension: "ogg",
        encoder: &["-c:a", "libvorbis"],
        tag_type: TagType::VorbisComments,
    },
    Case {
        name: "mp3 id3v2 TSRC",
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
        name: "m4a freeform ISRC atom",
        extension: "m4a",
        encoder: &["-c:a", "aac"],
        tag_type: TagType::Mp4Ilst,
    },
    Case {
        name: "wav id3v2 chunk TSRC",
        extension: "wav",
        encoder: &["-c:a", "pcm_s16le"],
        tag_type: TagType::Id3v2,
    },
];

async fn observed_isrc(case: &Case, with_isrc: bool) -> Option<String> {
    let (dir, hito, actor, library, music) = support::library("isrc-tag-host").await;
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
    if with_isrc {
        assert!(tag.insert_text(ItemKey::Isrc, ISRC.into()));
    }
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
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
    tags.isrc.map(|tag| tag.value)
}

#[tokio::test]
async fn an_isrc_tag_is_retained_as_a_source_assertion_in_every_supported_dialect() {
    let mut failures = Vec::new();
    for case in &CASES {
        let observed = observed_isrc(case, true).await;
        if observed.as_deref() != Some(ISRC) {
            failures.push(format!("{}: {observed:?}", case.name));
        }
        assert_eq!(observed_isrc(case, false).await, None, "{}", case.name);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

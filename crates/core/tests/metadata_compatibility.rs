use hito_core::test_support::support;

#[path = "fixtures/id3_tags.rs"]
mod id3;

use hito_core::{MetadataHistoryQuery, Search};
use id3::{Frame, Version, tag, utf16_payload};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::{path::Path, process::Command};

const TITLE: &str = "Étoile du soir";
const ARTIST: &str = "Artiste Ünïcode";
const ALBUM: &str = "Album — complet";
const ALBUM_ARTIST: &str = "Groupe Ω";

#[derive(Clone, Copy)]
enum Writer {
    Lofty(TagType, bool),
    Ffmpeg(&'static [&'static str]),
    HandBuilt(Version, bool),
}

struct Case {
    name: &'static str,
    extension: &'static str,
    encoder: &'static [&'static str],
    writer: Writer,
    expects_album_artist_and_disc: bool,
}

const MP3: &[&str] = &[
    "-c:a",
    "libmp3lame",
    "-write_id3v2",
    "0",
    "-write_xing",
    "0",
];

fn cases() -> Vec<Case> {
    let case = |name, extension, encoder, writer, full| Case {
        name,
        extension,
        encoder,
        writer,
        expects_album_artist_and_disc: full,
    };
    vec![
        case(
            "mp3 id3v2.3 (hand built, utf-16)",
            "mp3",
            MP3,
            Writer::HandBuilt(Version::V23, true),
            true,
        ),
        case(
            "mp3 id3v2.4 (hand built, utf-8)",
            "mp3",
            MP3,
            Writer::HandBuilt(Version::V24, false),
            true,
        ),
        case(
            "mp3 id3v2.3 (lofty)",
            "mp3",
            MP3,
            Writer::Lofty(TagType::Id3v2, true),
            true,
        ),
        case(
            "mp3 id3v2.4 (lofty)",
            "mp3",
            MP3,
            Writer::Lofty(TagType::Id3v2, false),
            true,
        ),
        case(
            "mp3 id3v2.3 (ffmpeg)",
            "mp3",
            MP3,
            Writer::Ffmpeg(&["-id3v2_version", "3", "-write_id3v2", "1"]),
            true,
        ),
        case(
            "mp3 id3v2.4 (ffmpeg)",
            "mp3",
            MP3,
            Writer::Ffmpeg(&["-id3v2_version", "4", "-write_id3v2", "1"]),
            true,
        ),
        case(
            "mp3 id3v1 (lofty)",
            "mp3",
            MP3,
            Writer::Lofty(TagType::Id3v1, false),
            false,
        ),
        case(
            "aac adts + id3v2 (ffmpeg)",
            "aac",
            &["-c:a", "aac"],
            Writer::Ffmpeg(&["-write_id3v2", "1"]),
            true,
        ),
        case(
            "flac vorbis comments (lofty)",
            "flac",
            &["-c:a", "flac"],
            Writer::Lofty(TagType::VorbisComments, false),
            true,
        ),
        case(
            "flac vorbis comments (ffmpeg)",
            "flac",
            &["-c:a", "flac"],
            Writer::Ffmpeg(&[]),
            true,
        ),
        case(
            "ogg vorbis (lofty)",
            "ogg",
            &["-c:a", "libvorbis"],
            Writer::Lofty(TagType::VorbisComments, false),
            true,
        ),
        case(
            "ogg vorbis (ffmpeg)",
            "ogg",
            &["-c:a", "libvorbis"],
            Writer::Ffmpeg(&[]),
            true,
        ),
        case(
            "opus (lofty)",
            "opus",
            &["-c:a", "libopus"],
            Writer::Lofty(TagType::VorbisComments, false),
            true,
        ),
        case(
            "opus (ffmpeg)",
            "opus",
            &["-c:a", "libopus"],
            Writer::Ffmpeg(&[]),
            true,
        ),
        case(
            "m4a ilst atoms (lofty)",
            "m4a",
            &["-c:a", "aac"],
            Writer::Lofty(TagType::Mp4Ilst, false),
            true,
        ),
        case(
            "m4a ilst atoms (ffmpeg)",
            "m4a",
            &["-c:a", "aac"],
            Writer::Ffmpeg(&[]),
            true,
        ),
        case(
            "mp4 ilst atoms (ffmpeg)",
            "mp4",
            &["-c:a", "aac"],
            Writer::Ffmpeg(&[]),
            true,
        ),
        case(
            "wav id3v2.4 chunk (lofty)",
            "wav",
            &["-c:a", "pcm_s16le"],
            Writer::Lofty(TagType::Id3v2, false),
            true,
        ),
        case(
            "wav id3v2.3 chunk (lofty)",
            "wav",
            &["-c:a", "pcm_s16le"],
            Writer::Lofty(TagType::Id3v2, true),
            true,
        ),
        case(
            "wav LIST INFO (lofty)",
            "wav",
            &["-c:a", "pcm_s16le"],
            Writer::Lofty(TagType::RiffInfo, false),
            false,
        ),
        case(
            "wav LIST INFO (ffmpeg)",
            "wav",
            &["-c:a", "pcm_s16le"],
            Writer::Ffmpeg(&[]),
            false,
        ),
        case(
            "aiff id3 chunk (lofty)",
            "aiff",
            &["-c:a", "pcm_s16be"],
            Writer::Lofty(TagType::Id3v2, false),
            true,
        ),
        case(
            "aiff id3 chunk (ffmpeg)",
            "aiff",
            &["-c:a", "pcm_s16be"],
            Writer::Ffmpeg(&["-write_id3v2", "1"]),
            true,
        ),
        case(
            "wma asf attributes (ffmpeg)",
            "wma",
            &["-c:a", "wmav2"],
            Writer::Ffmpeg(&[]),
            true,
        ),
    ]
}

fn ffmpeg(arguments: &[&str], input: &Path, output: &Path) {
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(input)
        .args(arguments)
        .arg(output)
        .status()
        .unwrap();
    assert!(status.success());
}

fn lofty_tag(tag_type: TagType, full: bool) -> Tag {
    let mut tag = Tag::new(tag_type);
    tag.set_title(TITLE.into());
    tag.set_artist(ARTIST.into());
    tag.set_album(ALBUM.into());
    tag.set_track(5);
    if full {
        tag.set_disk(2);
        tag.insert_text(ItemKey::AlbumArtist, ALBUM_ARTIST.into());
    }
    tag
}

fn hand_built(version: Version, utf16: bool) -> Vec<u8> {
    let text = |id, value: &str, ascii: bool| {
        if utf16 && !ascii {
            Frame::plain(id, utf16_payload(value))
        } else {
            Frame::text(id, value)
        }
    };
    let frames = [
        text("TIT2", TITLE, false),
        text("TPE1", ARTIST, false),
        text("TALB", ALBUM, false),
        text("TPE2", ALBUM_ARTIST, false),
        text("TPOS", "2/3", true),
        text("TRCK", "5/12", true),
    ];
    tag(version, false, &frames)
}

fn write(case: &Case, scratch: &Path, music: &Path) {
    let source = scratch.join("source.wav");
    support::write_wav(&source, 0);
    let path = music.join(format!("{}.{}", "file-name-fallback", case.extension));
    let metadata = [
        format!("title={TITLE}"),
        format!("artist={ARTIST}"),
        format!("album={ALBUM}"),
        format!("album_artist={ALBUM_ARTIST}"),
        "disc=2/3".to_owned(),
        "track=5/12".to_owned(),
    ];
    let mut encoder: Vec<&str> = case.encoder.to_vec();
    if let Writer::Ffmpeg(extra) = case.writer {
        for entry in &metadata {
            encoder.extend(["-metadata", entry]);
        }
        encoder.extend(extra);
    }
    ffmpeg(&encoder, &source, &path);
    match case.writer {
        Writer::Lofty(tag_type, v23) => {
            if tag_type == TagType::Id3v1 {
                TagType::Id3v2.remove_from_path(&path).unwrap();
            }
            let tag = lofty_tag(tag_type, case.expects_album_artist_and_disc);
            tag.save_to_path(&path, WriteOptions::default().use_id3v23(v23))
                .unwrap();
        }
        Writer::HandBuilt(version, utf16) => {
            let mut bytes = hand_built(version, utf16);
            bytes.extend(std::fs::read(&path).unwrap());
            std::fs::write(&path, bytes).unwrap();
        }
        Writer::Ffmpeg(_) => {}
    }
}

async fn read_back(case: &Case) -> Vec<String> {
    let (dir, hito, actor, library, music) = support::library("compatibility-host").await;
    write(case, dir.path(), &music);
    support::scan(&hito, &actor, &library).await;
    let page = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    if page.total != 1 {
        return vec![format!("file not indexed (total {})", page.total)];
    }
    let detail = hito
        .catalog
        .recording_metadata(&actor, &page.items[0].id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    let tags = detail.selected_observation.unwrap().tags.unwrap();
    let value = |tag: Option<hito_core::ObservedTag>| tag.map(|tag| tag.value);
    let number = |tag: Option<hito_core::ObservedTag>| {
        value(tag).and_then(|v| {
            v.split('/')
                .next()
                .and_then(|n| n.trim().parse::<u32>().ok())
        })
    };
    let mut problems = Vec::new();
    let title = value(tags.title);
    let artist = value(tags.artist);
    let album = value(tags.album);
    let track = number(tags.track);
    if title.as_deref() != Some(TITLE) {
        problems.push(format!("title {title:?}"));
    }
    if artist.as_deref() != Some(ARTIST) {
        problems.push(format!("artist {artist:?}"));
    }
    if album.as_deref() != Some(ALBUM) {
        problems.push(format!("album {album:?}"));
    }
    if track != Some(5) {
        problems.push(format!("track {track:?}"));
    }
    if case.expects_album_artist_and_disc {
        let album_artist = value(tags.album_artist);
        let disc = number(tags.disc);
        if album_artist.as_deref() != Some(ALBUM_ARTIST) {
            problems.push(format!("album artist {album_artist:?}"));
        }
        if disc != Some(2) {
            problems.push(format!("disc {disc:?}"));
        }
    }
    problems
}

#[tokio::test]
async fn every_supported_container_and_tag_dialect_yields_the_fields_hito_reads() {
    let mut failures = Vec::new();
    for case in cases() {
        let problems = read_back(&case).await;
        println!(
            "MATRIX {:<36} {}",
            case.name,
            if problems.is_empty() {
                "ok".into()
            } else {
                problems.join("; ")
            }
        );
        if !problems.is_empty() {
            failures.push(format!("{}: {}", case.name, problems.join("; ")));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

use serde_json::Value;
use std::{path::Path, process::Command};

pub const CREDIT: &str = "Shared source credit";
pub const ALBUM_CREDIT: &str = "Shared album credit";
pub const ASSERTIONS: [(&str, &str, &str); 4] = [
    (
        "MusicBrainz Artist Id",
        "artist",
        "10000000-0000-4000-8000-000000000001;10000000-0000-4000-8000-000000000002",
    ),
    (
        "MUSICBRAINZ_ARTISTID",
        "artist",
        "10000000-0000-4000-8000-000000000003",
    ),
    (
        "MusicBrainz Album Artist Id",
        "album_artist",
        "10000000-0000-4000-8000-000000000004",
    ),
    (
        "MUSICBRAINZ_ALBUMARTISTID",
        "album_artist",
        "10000000-0000-4000-8000-000000000005",
    ),
];

pub fn encode(input: &Path, output: &Path, title: &str) {
    let mut command = encoder(input);
    command.args(["-c:a", "flac", "-map_metadata", "-1"]);
    for value in [
        format!("title={title}"),
        format!("artist={CREDIT}"),
        "album=Owned album".into(),
        format!("album_artist={ALBUM_CREDIT}"),
    ] {
        command.args(["-metadata", &value]);
    }
    assert!(command.arg(output).status().unwrap().success());
}

pub fn add_assertions(input: &Path, staged: &Path) {
    let mut command = encoder(input);
    command.args(["-c:a", "copy", "-map_metadata", "0"]);
    for (key, _, value) in ASSERTIONS {
        command.args(["-metadata", &format!("{key}={value}")]);
    }
    assert!(command.arg(staged).status().unwrap().success());
    std::fs::rename(staged, input).unwrap();
}

pub fn add_artist_assertion(input: &Path, staged: &Path, value: &str) {
    let mut command = encoder(input);
    command.args(["-c:a", "copy", "-map_metadata", "0"]);
    command.args(["-metadata", &format!("MUSICBRAINZ_ARTISTID={value}")]);
    assert!(command.arg(staged).status().unwrap().success());
    std::fs::rename(staged, input).unwrap();
}

pub fn probe(path: &Path) -> Value {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format_tags:stream_tags",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn encoder(input: &Path) -> Command {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(input)
        .args(["-map", "0:a:0"]);
    command
}

use serde_json::Value;
use std::{path::Path, process::Command};

pub struct Probe {
    pub document: Value,
    pub output_bytes: usize,
}

pub fn add_artist_assertion(input: &Path, directory: &Path, ordinary: &[(&str, &str)], raw: &str) {
    let sidecar = directory.join("large-tags.ffmetadata");
    let staged = directory.join("large-tag-staged.flac");
    let mut metadata = String::from(";FFMETADATA1\n");
    for (key, value) in ordinary
        .iter()
        .copied()
        .chain(std::iter::once(("MUSICBRAINZ_ARTISTID", raw)))
    {
        metadata.push_str(key);
        metadata.push('=');
        metadata.push_str(&escaped(value));
        metadata.push('\n');
    }
    std::fs::write(&sidecar, metadata).unwrap();
    let output = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(input)
        .args(["-f", "ffmetadata", "-i"])
        .arg(sidecar)
        .args(["-map", "0:a:0", "-c:a", "copy", "-map_metadata", "1"])
        .arg(&staged)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Owned sidecar retagging must succeed"
    );
    assert!(
        output.stderr.is_empty(),
        "Owned sidecar retagging must have no diagnostics"
    );
    std::fs::rename(staged, input).unwrap();
}

pub fn probe(path: &Path, expanded: bool) -> Probe {
    let ordinary = "title,artist,album,album_artist,disc,track,isrc,TSRC";
    let tags = if expanded {
        format!(
            "{ordinary},MusicBrainz Artist Id,MUSICBRAINZ_ARTISTID,\
             MusicBrainz Album Artist Id,MUSICBRAINZ_ALBUMARTISTID"
        )
    } else {
        ordinary.to_owned()
    };
    let entries = format!(
        "stream=codec_name,sample_rate,channels,channel_layout,bits_per_sample,bits_per_raw_sample:\
         format=duration,format_name:format_tags={tags}:stream_tags={tags}"
    );
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            &entries,
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Owned metadata inspection must succeed"
    );
    assert!(
        output.stderr.is_empty(),
        "Owned metadata inspection must have no diagnostics"
    );
    Probe {
        document: serde_json::from_slice(&output.stdout).unwrap(),
        output_bytes: output.stdout.len(),
    }
}

fn escaped(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\\n")
        .replace('=', "\\=")
        .replace(';', "\\;")
        .replace('#', "\\#")
}

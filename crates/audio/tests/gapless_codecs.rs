use hito_audio::Decoder;
use std::{fs::File, path::Path, process::Command};

const FRAMES: usize = 24_000;

fn encode(path: &Path, codec: &str, extra: &[&str]) {
    let result = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=0.5",
            "-c:a",
            codec,
        ])
        .args(extra)
        .arg("-y")
        .arg(path)
        .output()
        .unwrap();
    assert!(result.status.success(), "fixture encoder {codec} failed");
}

fn decoded_frames(path: &Path) -> usize {
    let mut decoder = Decoder::open(File::open(path).unwrap()).unwrap();
    let mut frames = 0;
    while let Some(block) = decoder.read().unwrap() {
        frames += block.samples.len();
    }
    frames
}

#[test]
fn lossless_and_gapless_tagged_streams_decode_to_exactly_the_encoded_length() {
    let root = tempfile::tempdir().unwrap();
    for (name, codec, extra) in [
        ("exact.wav", "pcm_s16le", vec![]),
        ("exact.flac", "flac", vec![]),
        ("exact.m4a", "alac", vec![]),
        ("lame.mp3", "libmp3lame", vec![]),
        ("exact.ogg", "libvorbis", vec![]),
    ] {
        let path = root.path().join(name);
        encode(&path, codec, &extra);
        assert_eq!(decoded_frames(&path), FRAMES, "{name} is not sample exact");
    }
}

#[test]
fn encoder_delay_and_padding_without_usable_metadata_remain_in_the_decoded_stream() {
    let root = tempfile::tempdir().unwrap();
    for (name, codec, extra, limit) in [
        (
            "untagged.mp3",
            "libmp3lame",
            vec!["-write_xing", "0"],
            1_700,
        ),
        ("priming.m4a", "aac", vec![], 2_112),
        ("priming.aac", "aac", vec!["-f", "adts"], 2_112),
    ] {
        let path = root.path().join(name);
        encode(&path, codec, &extra);
        let extra_frames = decoded_frames(&path) - FRAMES;
        assert!(
            (1..=limit).contains(&extra_frames),
            "{name} carried {extra_frames} extra frames"
        );
    }
}

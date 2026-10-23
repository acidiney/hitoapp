use hito_audio::Decoder;
use std::{io::Cursor, time::Duration};

pub(super) async fn fixture() -> Vec<u8> {
    encoded(2, None).await
}

pub(super) async fn encoded(channels: u16, id3_version: Option<u8>) -> Vec<u8> {
    assert!(matches!(channels, 1 | 2));
    assert!(matches!(id3_version, None | Some(0 | 3 | 4)));
    let root = tempfile::tempdir().expect("owned fixture directory");
    let path = root.path().join("owned.mp3");
    let mut command = tokio::process::Command::new("ffmpeg");
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "2",
            "-ac",
        ])
        .arg(channels.to_string())
        .args([
            "-c:a",
            "libmp3lame",
            "-b:a",
            "128k",
            "-threads",
            "1",
            "-map_metadata",
            "-1",
        ]);
    if let Some(version) = id3_version {
        command.arg("-id3v2_version").arg(version.to_string());
    }
    let encoded = tokio::time::timeout(
        Duration::from_secs(5),
        command.arg("-y").arg(&path).kill_on_drop(true).output(),
    )
    .await
    .expect("owned encoding deadline")
    .expect("owned FFmpeg process");
    assert!(encoded.status.success(), "owned MP3 encoding must succeed");
    let bytes = std::fs::read(path).expect("owned MP3 bytes");
    assert!(
        (1000..=65_536).contains(&bytes.len()),
        "owned encoded size bound"
    );
    bytes
}

pub(super) fn decode(bytes: &[u8]) -> Vec<f32> {
    decode_profile(bytes, 2)
}

pub(super) fn id3_footer(tagless: &[u8]) -> Vec<u8> {
    assert!(tagless.len() >= 4 && tagless[0] == 0xff && tagless[1] & 0xe0 == 0xe0);
    let text = b"\0Owned MP3 fixture\0";
    assert!(text.len() < 128);
    let mut frame = b"TSSE".to_vec();
    frame.extend_from_slice(&(text.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(text);
    assert!(frame.len() < 128);
    let mut header = b"ID3\x04\x00\x10\0\0\0\0".to_vec();
    header[9] = frame.len() as u8;
    let mut footer = header.clone();
    footer[..3].copy_from_slice(b"3DI");
    let audio_offset = header.len() + frame.len() + footer.len();
    let mut encoded = header;
    encoded.extend_from_slice(&frame);
    encoded.extend_from_slice(&footer);
    encoded.extend_from_slice(tagless);
    assert!(
        encoded[audio_offset..] == *tagless,
        "footer fixture must preserve MPEG bytes"
    );
    encoded
}

pub(super) fn decode_profile(bytes: &[u8], channels: u16) -> Vec<f32> {
    assert!(matches!(channels, 1 | 2));
    let mut decoder = Decoder::open(Cursor::new(bytes.to_vec())).expect("owned MP3 must decode");
    assert_eq!(decoder.format().sample_rate, 48_000);
    assert_eq!(decoder.format().channels, channels);
    let limit = 100_000 * usize::from(channels);
    let minimum = 95_000 * usize::from(channels);
    let mut samples = Vec::new();
    while let Some(block) = decoder.read().expect("owned MP3 must decode through EOF") {
        assert!(
            block.samples.len() <= limit.saturating_sub(samples.len()),
            "owned PCM bound"
        );
        assert!(block.samples.iter().all(|sample| sample.is_finite()));
        samples.extend_from_slice(block.samples);
    }
    assert!(
        (minimum..=limit).contains(&samples.len()),
        "owned MP3 duration bound"
    );
    let energy = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>();
    assert!(
        (0.001..0.02).contains(&(energy / samples.len() as f64)),
        "owned sine signal must remain audible"
    );
    samples
}

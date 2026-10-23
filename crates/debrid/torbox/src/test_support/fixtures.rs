use crate::TorrentFileBinding;
use hito_testkit::audio::{RampVerifier, pcm16_wav, ramp_sample};
use serde_json::json;

pub const TOKEN: &str = "owned-torbox-key/+?=&-kept-private";
pub const MEDIA_HOST: &str = "media.hito-fixture.invalid";
pub const MEDIA_TARGET: &str = "/owned.wav?signature=owned-private-media-signature";
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
pub const FRAMES: u64 = 16_384;
pub const WAV_BYTES: u64 = 44 + FRAMES * CHANNELS as u64 * 2;

pub fn binding() -> TorrentFileBinding {
    TorrentFileBinding {
        torrent_id: 7123,
        info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
        torrent_bytes: WAV_BYTES * 2 + 400,
        file_id: 7,
        file_name: "album/owned.wav".into(),
        file_bytes: WAV_BYTES,
    }
}

pub fn observed(finished: bool, present: bool, selected_name: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "success": true, "error": null, "detail": "owned-private-observation-detail",
        "data": { "id": 7123, "hash": "0123456789abcdef0123456789abcdef01234567",
            "size": WAV_BYTES * 2 + 400, "download_finished": finished,
            "download_present": present, "download_state": "completed",
            "name": "owned-private-torrent-title", "files": [
                { "id": 8, "name": "album/owned.wav", "size": WAV_BYTES },
                { "id": 7, "name": selected_name, "size": WAV_BYTES },
                { "id": 9, "name": "private-notes.txt", "size": 400 }
            ] }
    }))
    .unwrap()
}

pub fn sample(index: usize) -> i16 {
    ramp_sample(index)
}

pub fn wave() -> Vec<u8> {
    pcm16_wav(SAMPLE_RATE, CHANNELS, FRAMES, sample)
}

pub fn acknowledgment() -> Vec<u8> {
    serde_json::to_vec(&json!({ "success": true, "error": null,
        "data": format!("https://{MEDIA_HOST}{MEDIA_TARGET}") }))
    .unwrap()
}

pub fn assert_pcm(bytes: Vec<u8>) {
    let mut decoder = hito_audio::Decoder::open(std::io::Cursor::new(bytes)).unwrap();
    let mut verifier = RampVerifier::new(FRAMES * u64::from(CHANNELS));
    while let Some(block) = decoder.read().unwrap() {
        verifier.verify(block.samples);
    }
    verifier.finish();
}

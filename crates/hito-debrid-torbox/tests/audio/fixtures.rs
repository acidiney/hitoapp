use hito_debrid_torbox::TorrentFileBinding;
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
    let frame = index / usize::from(CHANNELS);
    let value = ((frame % 257) as i16 - 128) * 128;
    if index.is_multiple_of(usize::from(CHANNELS)) {
        value
    } else {
        -value
    }
}

pub fn wave() -> Vec<u8> {
    let data_bytes = (WAV_BYTES - 44) as u32;
    let mut bytes = Vec::with_capacity(WAV_BYTES as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(data_bytes + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&CHANNELS.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * u32::from(CHANNELS) * 2).to_le_bytes());
    bytes.extend_from_slice(&(CHANNELS * 2).to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for index in 0..(FRAMES * u64::from(CHANNELS)) as usize {
        bytes.extend_from_slice(&sample(index).to_le_bytes());
    }
    bytes
}

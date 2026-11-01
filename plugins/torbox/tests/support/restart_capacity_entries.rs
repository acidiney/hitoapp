use super::fixtures;
use hito_debrid_torbox::TorrentFileBinding;
use hito_plugin_sdk::{PreparationRequest, ProviderRecording};
use hito_plugin_torbox::CuratedWavRecording;
use std::ops::Range;
use uuid::Uuid;

pub const RECORDING: &str = "retained-capacity-recording";
pub const SOURCE: &str = "retained-capacity-source";
pub const FIRST_FILLER_TORRENT: u64 = 20_000;

pub fn batch(number: usize) -> Range<usize> {
    let start = number
        .checked_mul(127)
        .expect("the owned catalog batch must be bounded");
    assert!(start < 1023);
    start..(start + 127).min(1023)
}

pub fn catalog(number: usize) -> Vec<CuratedWavRecording> {
    std::iter::once(recording(
        RECORDING.into(),
        SOURCE.into(),
        fixtures::binding(),
    ))
    .chain(
        batch(number).map(|index| recording(recording_id(index), source_id(index), binding(index))),
    )
    .collect()
}

pub fn ready_request() -> PreparationRequest {
    PreparationRequest {
        operation_id: Uuid::new_v4(),
        recording_id: RECORDING.into(),
        source_id: SOURCE.into(),
    }
}

pub fn request(index: usize) -> PreparationRequest {
    PreparationRequest {
        operation_id: Uuid::new_v4(),
        recording_id: recording_id(index),
        source_id: source_id(index),
    }
}

pub fn binding(index: usize) -> TorrentFileBinding {
    assert!(index < 1023);
    let mut binding = fixtures::binding();
    binding.torrent_id = FIRST_FILLER_TORRENT + index as u64;
    binding
}

fn recording_id(index: usize) -> String {
    format!("capacity-filler-recording-{index:04}")
}
fn source_id(index: usize) -> String {
    format!("capacity-filler-source-{index:04}")
}

fn recording(id: String, source_id: String, binding: TorrentFileBinding) -> CuratedWavRecording {
    CuratedWavRecording {
        recording: ProviderRecording {
            isrc: None,
            artist_urls: Vec::new(),
            artists: Vec::new(),
            id,
            title: "owned-private-capacity-title".into(),
            artist: "owned-private-capacity-artist".into(),
            duration_ms: Some(fixtures::FRAMES * 1000 / u64::from(fixtures::SAMPLE_RATE)),
            release: None,
            disc_number: None,
            track_number: None,
            revision: None,
            attribution: None,
        },
        source_id,
        label: "Curated canonical PCM16 WAV".into(),
        binding,
    }
}

use super::fixtures;
use hito_debrid_torbox::TorrentFileBinding;
use hito_plugin_sdk::ProviderRecording;
use hito_plugin_torbox::CuratedWavRecording;

pub const RECORDING: &str = "retained-catalog-recording";
pub const SOURCE: &str = "retained-catalog-source";
pub const ADDED_RECORDING: &str = "aaa-unrelated-catalog-recording";
pub const ADDED_SOURCE: &str = "aaa-unrelated-catalog-source";
pub const MOVED_RECORDING: &str = "moved-catalog-recording";

#[derive(Clone, Copy)]
pub enum Evolution {
    Surrounded,
    Reordered,
    RemovedUnrelated,
    RemovedOriginal,
    Restored,
    ChangedBinding,
    MovedSource,
}

impl Evolution {
    pub fn recordings(self) -> Vec<CuratedWavRecording> {
        match self {
            Self::Surrounded => vec![unrelated(), retained(), other()],
            Self::Reordered => vec![other(), retained(), unrelated()],
            Self::RemovedUnrelated => vec![other(), retained()],
            Self::RemovedOriginal => vec![unrelated()],
            Self::Restored => original(true),
            Self::ChangedBinding => {
                let mut changed = retained();
                changed.binding.file_name = "album/changed-owned.wav".into();
                vec![unrelated(), changed]
            }
            Self::MovedSource => vec![
                unrelated(),
                recording(MOVED_RECORDING, SOURCE, fixtures::binding()),
            ],
        }
    }
}

pub fn original(include_unrelated: bool) -> Vec<CuratedWavRecording> {
    if include_unrelated {
        vec![unrelated(), retained()]
    } else {
        vec![retained()]
    }
}

fn retained() -> CuratedWavRecording {
    recording(RECORDING, SOURCE, fixtures::binding())
}

pub fn control_binding() -> TorrentFileBinding {
    let mut binding = fixtures::binding();
    binding.torrent_id = 8124;
    binding.info_hash = "89abcdef0123456789abcdef0123456789abcdef".into();
    binding.torrent_bytes = fixtures::WAV_BYTES * 3 + 400;
    binding.file_id = 19;
    binding.file_name = "unrelated/extra.wav".into();
    assert!(binding.validate().is_ok());
    binding
}

fn unrelated() -> CuratedWavRecording {
    recording(ADDED_RECORDING, ADDED_SOURCE, control_binding())
}

fn other() -> CuratedWavRecording {
    let mut binding = control_binding();
    binding.torrent_id = 9125;
    binding.file_id = 23;
    binding.file_name = "unrelated/another.wav".into();
    recording(
        "zzz-unrelated-catalog-recording",
        "zzz-unrelated-catalog-source",
        binding,
    )
}

fn recording(id: &str, source: &str, binding: TorrentFileBinding) -> CuratedWavRecording {
    CuratedWavRecording {
        recording: ProviderRecording {
            isrc: None,
            artist_urls: Vec::new(),
            id: id.into(),
            title: "owned-private-catalog-title".into(),
            artist: "owned-private-catalog-artist".into(),
            duration_ms: Some(fixtures::FRAMES * 1000 / u64::from(fixtures::SAMPLE_RATE)),
            release: None,
            disc_number: None,
            track_number: None,
            revision: None,
            attribution: None,
        },
        source_id: source.into(),
        label: "Curated canonical PCM16 WAV".into(),
        binding,
    }
}

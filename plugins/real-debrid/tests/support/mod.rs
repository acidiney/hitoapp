pub mod audio;
pub mod http;
pub mod logging;
#[allow(dead_code)]
pub mod media;
pub mod provider;

use hito_debrid_real_debrid::TorrentFileBinding;
use hito_plugin_sdk::ProviderRecording;

pub const PROVIDER_TOKEN: &str = "private-owned-rd-provider-token-canary";
pub const SERVICE_TOKEN: &str = "private-owned-rd-service-token-canary";
pub const WRONG_TOKEN: &str = "private-owned-rd-wrong-token-canary";
pub const COOKIE: &str = "private-owned-browser-cookie-canary";
pub const QUERY: &str = "owned-title-canary";
pub const TITLE: &str = "private-owned-title-canary";
pub const ARTIST: &str = "private-owned-artist-canary";
pub const RECORDING: &str = "curated-recording-1";
pub const SOURCE: &str = "curated-wav-source-1";
pub const LABEL: &str = "Curated canonical PCM16 WAV";
pub const TORRENT: &str = "private-owned-torrent-canary";
pub const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
pub const FILE: &str = "/private-owned-directory/private-owned-file-canary.wav";
pub const API_HOST: &str = "api.real-debrid.com";
pub const MEDIA_HOST: &str = "media.hito-fixture.invalid";
pub const HOSTER_LINK: &str = "https://hoster.hito-fixture.invalid/private-original-link?signature=private-original-signature&part=one+two";
pub const MEDIA_TARGET: &str = "/private-media-canary.wav?signature=private-media-signature-canary";
pub const EXPECTED_FORM: &str = "link=https%3A%2F%2Fhoster.hito-fixture.invalid%2Fprivate-original-link%3Fsignature%3Dprivate-original-signature%26part%3Done%2Btwo";

pub fn binding() -> TorrentFileBinding {
    TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: audio::WAV_BYTES as u64,
    }
}

pub fn recording() -> ProviderRecording {
    ProviderRecording {
        isrc: None,
        id: RECORDING.into(),
        title: TITLE.into(),
        artist: ARTIST.into(),
        duration_ms: Some(2_000),
        release: None,
        disc_number: None,
        track_number: None,
        revision: Some("curated-wav-v1".into()),
        attribution: None,
    }
}

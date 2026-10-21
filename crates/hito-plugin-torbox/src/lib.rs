mod audio;
mod catalog;
mod http;
mod policy;
mod preparation;

use hito_debrid_torbox::{MediaAllowlist, TorBoxClient, TorrentFileBinding};
use hito_plugin_sdk::ProviderRecording;

pub struct Config {
    pub service_bearer: String,
    pub client: TorBoxClient,
    pub media_origins: MediaAllowlist,
    pub recordings: Vec<CuratedWavRecording>,
}

/// One operator-reviewed recording and its exact canonical PCM16 WAV selection.
pub struct CuratedWavRecording {
    pub recording: ProviderRecording,
    pub source_id: String,
    pub label: String,
    pub binding: TorrentFileBinding,
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("The plugin-service credential is invalid")]
    InvalidServiceCredential,
    #[error("The curated canonical-WAV catalog is invalid")]
    InvalidCatalog,
}

pub fn router(config: Config) -> Result<axum::Router, InitError> {
    http::router(config)
}

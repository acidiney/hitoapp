use crate::{auth::Authentication, catalog::Catalog};
use hito_debrid_real_debrid::{AudioKind, MediaAllowlist, RealDebridClient, TorrentFileBinding};
use hito_plugin_sdk::ProviderRecording;

pub struct Config {
    pub(crate) authentication: Authentication,
    pub(crate) client: RealDebridClient,
    pub(crate) media_origins: MediaAllowlist,
    pub(crate) catalog: Catalog,
}

/// One operator-reviewed recording and its declared encoded file.
pub struct CuratedAudioRecording {
    pub recording: ProviderRecording,
    pub source_id: String,
    pub label: String,
    pub binding: TorrentFileBinding,
    pub audio_kind: AudioKind,
}

impl Config {
    pub fn new(
        service_bearer: String,
        client: RealDebridClient,
        media_origins: MediaAllowlist,
        recordings: Vec<CuratedAudioRecording>,
    ) -> Result<Self, InitError> {
        let authentication = Authentication::new(service_bearer)?;
        let catalog = Catalog::new(recordings)?;
        Ok(Self {
            authentication,
            client,
            media_origins,
            catalog,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("The plugin-service credential is invalid")]
    InvalidServiceCredential,
    #[error("The curated audio catalog is invalid")]
    InvalidCatalog,
}

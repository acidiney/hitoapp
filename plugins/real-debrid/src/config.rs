use crate::catalog::Catalog;
use hito_debrid_real_debrid::{AudioKind, MediaAllowlist, RealDebridClient, TorrentFileBinding};
use hito_plugin_sdk::{ProviderRecording, server::ServiceAuth};
use zeroize::Zeroizing;

pub struct Config {
    pub(crate) authentication: ServiceAuth,
    pub(crate) client: RealDebridClient,
    pub(crate) media_origins: MediaAllowlist,
    pub(crate) catalog: Catalog,
}

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
        let service_bearer = Zeroizing::new(service_bearer);
        let authentication =
            ServiceAuth::new(&service_bearer).ok_or(InitError::InvalidServiceCredential)?;
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

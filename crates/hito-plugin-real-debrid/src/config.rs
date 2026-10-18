use crate::{auth::Authentication, catalog::Catalog};
use hito_debrid_real_debrid::{MediaAllowlist, RealDebridClient, TorrentFileBinding};
use hito_plugin_sdk::{ProviderRecording, ProviderSource, SourcePage};
use std::collections::HashSet;

const MAX_RECORDINGS: usize = 128;
const MAX_METADATA_BYTES: usize = 49_152;

pub struct Config {
    pub(crate) authentication: Authentication,
    pub(crate) client: RealDebridClient,
    pub(crate) media_origins: MediaAllowlist,
    pub(crate) catalog: Catalog,
}

/// One operator-reviewed recording and its exact canonical PCM16 WAV file.
pub struct CuratedWavRecording {
    pub recording: ProviderRecording,
    pub source_id: String,
    pub label: String,
    pub binding: TorrentFileBinding,
}

impl Config {
    pub fn new(
        service_bearer: String,
        client: RealDebridClient,
        media_origins: MediaAllowlist,
        recordings: Vec<CuratedWavRecording>,
    ) -> Result<Self, InitError> {
        let authentication = Authentication::new(service_bearer)?;
        if recordings.is_empty() || recordings.len() > MAX_RECORDINGS {
            return Err(InitError::InvalidCatalog);
        }
        let mut recording_ids = HashSet::new();
        let mut source_ids = HashSet::new();
        let mut metadata_bytes = 0_usize;
        for entry in &recordings {
            entry
                .recording
                .validate()
                .map_err(|_| InitError::InvalidCatalog)?;
            entry
                .binding
                .validate()
                .map_err(|_| InitError::InvalidCatalog)?;
            SourcePage {
                items: vec![ProviderSource {
                    id: entry.source_id.clone(),
                    label: entry.label.clone(),
                    ready: false,
                }],
            }
            .validate()
            .map_err(|_| InitError::InvalidCatalog)?;
            if !recording_ids.insert(entry.recording.id.clone())
                || !source_ids.insert(entry.source_id.clone())
            {
                return Err(InitError::InvalidCatalog);
            }
            let encoded =
                serde_json::to_vec(&entry.recording).map_err(|_| InitError::InvalidCatalog)?;
            if encoded.len() > MAX_METADATA_BYTES.saturating_sub(metadata_bytes) {
                return Err(InitError::InvalidCatalog);
            }
            metadata_bytes += encoded.len();
        }
        Ok(Self {
            authentication,
            client,
            media_origins,
            catalog: Catalog::new(recordings),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("The plugin-service credential is invalid")]
    InvalidServiceCredential,
    #[error("The curated canonical-WAV catalog is invalid")]
    InvalidCatalog,
}

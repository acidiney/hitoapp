mod audio;
mod http;
mod preparation;
mod receipts;

pub use http::router;

use hito_debrid_all_debrid::{AllDebridClient, MagnetFileBinding, MediaAllowlist};
use hito_plugin_sdk::{
    CuratedEntryView, ProviderRecording, SearchPage, SearchRequest, admit_curated_catalog,
};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

pub struct Config {
    pub service_bearer: String,
    pub receipt_key: [u8; 32],
    pub client: AllDebridClient,
    pub media_origins: MediaAllowlist,
    pub recordings: Vec<CuratedWavRecording>,
}

/// An operator-curated recording and its exact canonical PCM16 WAV selection.
pub struct CuratedWavRecording {
    pub recording: ProviderRecording,
    pub source_id: String,
    pub label: String,
    pub binding: MagnetFileBinding,
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("The plugin-service credential is invalid")]
    InvalidServiceCredential,
    #[error("The private preparation key is invalid")]
    InvalidReceiptKey,
    #[error("The curated canonical-WAV catalog is invalid")]
    InvalidCatalog,
}

pub(crate) struct Entry {
    configured: CuratedWavRecording,
    witness: [u8; 32],
    slot: Mutex<preparation::Slot>,
}

pub(crate) struct Catalog {
    entries: Vec<Arc<Entry>>,
    claims: Mutex<()>,
}

impl Catalog {
    fn new(configured: Vec<CuratedWavRecording>) -> Result<Self, InitError> {
        let configured = admit_curated_catalog(configured, |entry| CuratedEntryView {
            recording: &entry.recording,
            source_id: &entry.source_id,
            label: &entry.label,
        })
        .map_err(|_| InitError::InvalidCatalog)?;
        let mut entries = Vec::with_capacity(configured.len());
        for value in configured {
            value
                .binding
                .validate()
                .map_err(|_| InitError::InvalidCatalog)?;
            let encoded = serde_json::to_vec(&(
                &value.recording,
                &value.source_id,
                &value.label,
                &value.binding,
            ))
            .map_err(|_| InitError::InvalidCatalog)?;
            entries.push(Arc::new(Entry {
                configured: value,
                witness: Sha256::digest(encoded).into(),
                slot: Mutex::new(preparation::Slot::default()),
            }));
        }
        Ok(Self {
            entries,
            claims: Mutex::new(()),
        })
    }

    pub(crate) fn recording(&self, id: &str) -> Result<Arc<Entry>, http::Error> {
        self.entries
            .iter()
            .find(|entry| entry.configured.recording.id == id)
            .cloned()
            .ok_or(http::Error::NotFound)
    }

    pub(crate) fn source(&self, id: &str) -> Result<Arc<Entry>, http::Error> {
        self.entries
            .iter()
            .find(|entry| entry.configured.source_id == id)
            .cloned()
            .ok_or(http::Error::NotFound)
    }

    pub(crate) fn search(&self, request: SearchRequest) -> Result<SearchPage, http::Error> {
        request
            .validate()
            .map_err(|_| http::Error::InvalidRequest)?;
        let offset = match request.cursor.as_deref() {
            None => 0,
            Some(value) => value
                .strip_prefix("offset:")
                .filter(|value| !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()))
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or(http::Error::InvalidRequest)?,
        };
        if offset > self.entries.len() {
            return Err(http::Error::InvalidRequest);
        }
        let query = request.query.to_lowercase();
        let mut matching = self
            .entries
            .iter()
            .filter(|entry| {
                entry
                    .configured
                    .recording
                    .title
                    .to_lowercase()
                    .contains(&query)
                    || entry
                        .configured
                        .recording
                        .artist
                        .to_lowercase()
                        .contains(&query)
            })
            .skip(offset);
        let items = matching
            .by_ref()
            .take(usize::from(request.limit))
            .map(|entry| entry.configured.recording.clone())
            .collect::<Vec<_>>();
        let next_cursor = matching
            .next()
            .map(|_| format!("offset:{}", offset + items.len()));
        Ok(SearchPage { items, next_cursor })
    }
}

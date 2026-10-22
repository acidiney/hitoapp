use crate::{CuratedWavRecording, InitError, policy::Error};
use hito_plugin_sdk::{CuratedEntryView, SearchPage, SearchRequest, admit_curated_catalog};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub(crate) struct Catalog {
    entries: Vec<CuratedWavRecording>,
    fingerprint: [u8; 32],
}

impl Catalog {
    pub(crate) fn new(entries: Vec<CuratedWavRecording>) -> Result<Self, InitError> {
        let entries = admit_curated_catalog(entries, |entry| CuratedEntryView {
            recording: &entry.recording,
            source_id: &entry.source_id,
            label: &entry.label,
        })
        .map_err(|_| InitError::InvalidConfiguration)?;
        let mut transfers = HashSet::new();
        let mut items = HashSet::new();
        let mut digest = Sha256::new();
        for entry in &entries {
            entry
                .binding
                .validate()
                .map_err(|_| InitError::InvalidConfiguration)?;
            if !transfers.insert(&entry.binding.transfer.transfer_id)
                || !items.insert(&entry.binding.item_id)
            {
                return Err(InitError::InvalidConfiguration);
            }
            let canonical = serde_json::to_vec(&(
                &entry.recording,
                &entry.source_id,
                &entry.label,
                &entry.binding,
            ))
            .map_err(|_| InitError::InvalidConfiguration)?;
            digest.update((canonical.len() as u64).to_be_bytes());
            digest.update(canonical);
        }
        Ok(Self {
            entries,
            fingerprint: digest.finalize().into(),
        })
    }

    pub(crate) fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    pub(crate) fn recording(&self, id: &str) -> Result<&CuratedWavRecording, Error> {
        self.entries
            .iter()
            .find(|entry| entry.recording.id == id)
            .ok_or(Error::NotFound)
    }

    pub(crate) fn source(&self, id: &str) -> Result<&CuratedWavRecording, Error> {
        self.entries
            .iter()
            .find(|entry| entry.source_id == id)
            .ok_or(Error::NotFound)
    }

    pub(crate) fn target(
        &self,
        recording: &str,
        source: &str,
    ) -> Result<&CuratedWavRecording, Error> {
        self.recording(recording).and_then(|entry| {
            if entry.source_id == source {
                Ok(entry)
            } else {
                Err(Error::NotFound)
            }
        })
    }

    pub(crate) fn search(&self, request: SearchRequest) -> Result<SearchPage, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        let offset = match request.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor
                .strip_prefix("offset:")
                .filter(|value| {
                    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                })
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or(Error::InvalidRequest)?,
        };
        if offset > self.entries.len() {
            return Err(Error::InvalidRequest);
        }
        let query = request.query.to_lowercase();
        let mut matching = self
            .entries
            .iter()
            .filter(|entry| {
                entry.recording.title.to_lowercase().contains(&query)
                    || entry.recording.artist.to_lowercase().contains(&query)
            })
            .skip(offset);
        let items: Vec<_> = matching
            .by_ref()
            .take(usize::from(request.limit))
            .map(|entry| entry.recording.clone())
            .collect();
        let next_cursor = matching
            .next()
            .map(|_| format!("offset:{}", offset + items.len()));
        Ok(SearchPage { items, next_cursor })
    }
}

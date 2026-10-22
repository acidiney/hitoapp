use crate::{CuratedAudioRecording, InitError, error::Error};
use hito_plugin_sdk::{
    CuratedEntryView, ProviderRecording, SearchPage, SearchRequest, admit_curated_catalog,
};

pub(crate) struct Catalog {
    entries: Vec<CuratedAudioRecording>,
}

impl Catalog {
    pub(crate) fn new(entries: Vec<CuratedAudioRecording>) -> Result<Self, InitError> {
        let entries = admit_curated_catalog(entries, |entry| CuratedEntryView {
            recording: &entry.recording,
            source_id: &entry.source_id,
            label: &entry.label,
        })
        .map_err(|_| InitError::InvalidCatalog)?;
        for entry in &entries {
            entry
                .binding
                .validate()
                .map_err(|_| InitError::InvalidCatalog)?;
        }
        Ok(Self { entries })
    }

    pub(crate) fn recording(&self, id: &str) -> Result<&CuratedAudioRecording, Error> {
        self.entries
            .iter()
            .find(|entry| entry.recording.id == id)
            .ok_or(Error::NotFound)
    }

    pub(crate) fn source(&self, id: &str) -> Result<&CuratedAudioRecording, Error> {
        self.entries
            .iter()
            .find(|entry| entry.source_id == id)
            .ok_or(Error::NotFound)
    }

    pub(crate) fn search(&self, request: SearchRequest) -> Result<SearchPage, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        let offset = match request.cursor.as_deref() {
            None => 0,
            Some(cursor) => {
                let offset = cursor
                    .strip_prefix("offset:")
                    .filter(|value| {
                        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                    })
                    .ok_or(Error::InvalidRequest)?;
                offset.parse::<usize>().map_err(|_| Error::InvalidRequest)?
            }
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
        let items: Vec<ProviderRecording> = matching
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

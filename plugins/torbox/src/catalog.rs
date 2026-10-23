use crate::{CuratedWavRecording, InitError, policy::Error};
use hito_plugin_sdk::{CuratedEntryView, SearchPage, SearchRequest, admit_curated_catalog};

pub(crate) struct Catalog {
    entries: Vec<CuratedWavRecording>,
}

impl Catalog {
    pub(crate) fn new(entries: Vec<CuratedWavRecording>) -> Result<Self, InitError> {
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

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn entry(&self, index: usize) -> &CuratedWavRecording {
        &self.entries[index]
    }
    pub(crate) fn recording(&self, id: &str) -> Result<usize, Error> {
        self.entries
            .iter()
            .position(|e| e.recording.id == id)
            .ok_or(Error::NotFound)
    }
    pub(crate) fn source(&self, id: &str) -> Result<usize, Error> {
        self.entries
            .iter()
            .position(|e| e.source_id == id)
            .ok_or(Error::NotFound)
    }
    pub(crate) fn target(&self, recording: &str, source: &str) -> Result<usize, Error> {
        let index = self.recording(recording)?;
        if self.entries[index].source_id != source {
            return Err(Error::NotFound);
        }
        Ok(index)
    }

    pub(crate) fn search(&self, request: SearchRequest) -> Result<SearchPage, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        let offset = match request.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor
                .strip_prefix("offset:")
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or(Error::InvalidRequest)?,
        };
        if offset > self.entries.len() {
            return Err(Error::InvalidRequest);
        }
        let query = request.query.to_lowercase();
        let mut matching = self
            .entries
            .iter()
            .filter(|e| {
                e.recording.title.to_lowercase().contains(&query)
                    || e.recording.artist.to_lowercase().contains(&query)
            })
            .skip(offset);
        let items: Vec<_> = matching
            .by_ref()
            .take(usize::from(request.limit))
            .map(|e| e.recording.clone())
            .collect();
        let next_cursor = matching
            .next()
            .map(|_| format!("offset:{}", offset + items.len()));
        Ok(SearchPage { items, next_cursor })
    }
}

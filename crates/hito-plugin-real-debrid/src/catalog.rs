use crate::{CuratedWavRecording, error::Error};
use hito_plugin_sdk::{ProviderRecording, SearchPage, SearchRequest};

pub(crate) struct Catalog {
    entries: Vec<CuratedWavRecording>,
}

impl Catalog {
    pub(crate) fn new(mut entries: Vec<CuratedWavRecording>) -> Self {
        entries.sort_by(|left, right| left.recording.id.cmp(&right.recording.id));
        Self { entries }
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

use crate::{CatalogSnapshot, CatalogSnapshotMode, CatalogSnapshotPage, Error, Result};
use std::collections::{HashMap, HashSet};

pub(super) const MAX_EXPORT_BYTES: usize = 128 * 1024 * 1024;
pub(super) const MAX_PROFILE_BYTES: i64 = 256 * 1024 * 1024;

pub(super) fn invalid() -> Error {
    Error::Upstream("The Hito server returned an invalid catalog snapshot")
}

#[derive(Default)]
pub(super) struct Validation {
    recordings: HashSet<String>,
    sources: HashSet<String>,
    releases: HashMap<String, (String, Option<String>)>,
    entries: HashSet<(String, String)>,
    bytes: usize,
}

impl Validation {
    pub fn metadata(metadata: &CatalogSnapshot, account: &str, library: &str) -> Result<()> {
        if metadata.account_id != account
            || metadata.library_id != library
            || metadata.mode != CatalogSnapshotMode::SnapshotOnly
            || metadata.collection_watermark < 0
            || metadata.recording_count > 100_000
            || metadata.page_count == 0
            || metadata.page_count > metadata.recording_count.max(1) as u64
            || (metadata.recording_count == 0 && metadata.page_count != 1)
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn page(
        &mut self,
        metadata: &CatalogSnapshot,
        ordinal: u64,
        page: &CatalogSnapshotPage,
        bytes: usize,
    ) -> Result<Vec<String>> {
        let next = if ordinal + 1 < metadata.page_count {
            Some(ordinal + 1)
        } else {
            None
        };
        if page.snapshot_id != metadata.id
            || page.ordinal != ordinal
            || page.next_ordinal != next
            || page.recordings.len() > 200
            || (page.recordings.is_empty() && metadata.recording_count != 0)
            || bytes > 256 * 1024
            || bytes == 0
        {
            return Err(invalid());
        }
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|total| *total <= MAX_EXPORT_BYTES)
            .ok_or_else(invalid)?;
        let mut payloads = Vec::with_capacity(page.recordings.len());
        for fact in &page.recordings {
            if fact.library_id != metadata.library_id
                || fact.duration_ms < 0
                || fact.sources.is_empty()
                || fact.sources.len() > 128
                || !self.recordings.insert(fact.id.clone())
                || self.recordings.len() > metadata.recording_count
            {
                return Err(invalid());
            }
            let payload = serde_json::to_string(fact).map_err(|_| invalid())?;
            if payload.len() > 64 * 1024 {
                return Err(invalid());
            }
            for source in &fact.sources {
                if source.duration_ms < 0 || !self.sources.insert(source.id.clone()) {
                    return Err(invalid());
                }
                if let Some(release) = &source.release {
                    if release.disc_number.is_some_and(|number| number <= 0)
                        || release.track_number.is_some_and(|number| number <= 0)
                    {
                        return Err(invalid());
                    }
                    if !self
                        .entries
                        .insert((release.id.clone(), release.entry_id.clone()))
                    {
                        return Err(invalid());
                    }
                    let fields = (release.title.clone(), release.artist.clone());
                    if self
                        .releases
                        .get(&release.id)
                        .is_some_and(|existing| *existing != fields)
                    {
                        return Err(invalid());
                    }
                    self.releases.entry(release.id.clone()).or_insert(fields);
                }
            }
            payloads.push(payload);
        }
        Ok(payloads)
    }

    pub fn complete(&self, metadata: &CatalogSnapshot) -> Result<()> {
        if self.recordings.len() != metadata.recording_count {
            return Err(invalid());
        }
        Ok(())
    }
}

use crate::{MAX_RECORDING_METADATA_BYTES, ProviderRecording, ProviderSource, SourcePage};
use std::{collections::HashSet, fmt};

pub const MAX_CURATED_ENTRIES: usize = 128;
pub const MAX_CURATED_CATALOG_BYTES: usize = 49_152;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuratedCatalogError {
    EntryCount,
    InvalidRecording,
    InvalidSource,
    DuplicateRecording,
    DuplicateSource,
    RecordingTooLarge,
    CatalogTooLarge,
}

impl fmt::Display for CuratedCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EntryCount => "The curated catalog must contain 1 to 128 entries",
            Self::InvalidRecording => "A curated recording is invalid",
            Self::InvalidSource => "A curated source is invalid",
            Self::DuplicateRecording => "The curated catalog repeats a recording ID",
            Self::DuplicateSource => "The curated catalog repeats a source ID",
            Self::RecordingTooLarge => "A curated recording exceeds the detail budget",
            Self::CatalogTooLarge => "The curated catalog exceeds its aggregate budget",
        })
    }
}

impl std::error::Error for CuratedCatalogError {}

pub struct CuratedEntryView<'a> {
    pub recording: &'a ProviderRecording,
    pub source_id: &'a str,
    pub label: &'a str,
}

pub fn admit_curated_catalog<E>(
    mut entries: Vec<E>,
    view: impl for<'a> Fn(&'a E) -> CuratedEntryView<'a>,
) -> Result<Vec<E>, CuratedCatalogError> {
    if entries.is_empty() || entries.len() > MAX_CURATED_ENTRIES {
        return Err(CuratedCatalogError::EntryCount);
    }
    entries.sort_by(|left, right| view(left).recording.id.cmp(&view(right).recording.id));
    let mut recording_ids = HashSet::new();
    let mut source_ids = HashSet::new();
    let mut catalog_bytes = 0_usize;
    for entry in &entries {
        let CuratedEntryView {
            recording,
            source_id,
            label,
        } = view(entry);
        recording
            .validate()
            .map_err(|_| CuratedCatalogError::InvalidRecording)?;
        SourcePage {
            items: vec![ProviderSource {
                id: source_id.to_owned(),
                label: label.to_owned(),
                ready: false,
            }],
        }
        .validate()
        .map_err(|_| CuratedCatalogError::InvalidSource)?;
        if !recording_ids.insert(recording.id.as_str()) {
            return Err(CuratedCatalogError::DuplicateRecording);
        }
        if !source_ids.insert(source_id) {
            return Err(CuratedCatalogError::DuplicateSource);
        }
        let detail_bytes =
            serde_json::to_vec(recording).map_err(|_| CuratedCatalogError::InvalidRecording)?;
        if detail_bytes.len() > MAX_RECORDING_METADATA_BYTES {
            return Err(CuratedCatalogError::RecordingTooLarge);
        }
        if detail_bytes.len() > MAX_CURATED_CATALOG_BYTES - catalog_bytes {
            return Err(CuratedCatalogError::CatalogTooLarge);
        }
        catalog_bytes += detail_bytes.len();
    }
    Ok(entries)
}

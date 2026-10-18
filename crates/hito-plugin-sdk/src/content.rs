use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const SEARCH_PATH: &str = "v1/search";
pub const DETAIL_PATH: &str = "v1/recordings/detail";
pub const SOURCES_PATH: &str = "v1/sources";
pub const AUDIO_PATH: &str = "v1/audio";
pub const MAX_SOURCES_PER_RECORDING: usize = 16;

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    pub cursor: Option<String>,
    pub limit: u16,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct SearchPage {
    pub items: Vec<ProviderRecording>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ProviderRecording {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: Option<u64>,
    pub release: Option<ProviderRelease>,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub revision: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ProviderRelease {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RecordingRequest {
    pub recording_id: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct SourcePage {
    pub items: Vec<ProviderSource>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ProviderSource {
    pub id: String,
    pub label: String,
    pub ready: bool,
}

impl RecordingRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.recording_id, 256)
    }
}

impl SourcePage {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.items.len() > MAX_SOURCES_PER_RECORDING {
            return Err("The provider returned too many sources");
        }
        let mut ids = HashSet::new();
        for source in &self.items {
            text(&source.id, 256)?;
            text(&source.label, 256)?;
            if !ids.insert(&source.id) {
                return Err("The provider returned duplicate source IDs");
            }
        }
        Ok(())
    }
}

impl SearchRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.query, 512)?;
        optional(self.cursor.as_deref(), 2048)?;
        if !(1..=50).contains(&self.limit) {
            return Err("Use a search limit between 1 and 50");
        }
        Ok(())
    }
}

impl SearchPage {
    pub fn validate(&self, requested: u16) -> Result<(), &'static str> {
        if self.items.len() > usize::from(requested.min(50)) {
            return Err("The provider returned too many search results");
        }
        optional(self.next_cursor.as_deref(), 2048)?;
        let mut ids = HashSet::new();
        for item in &self.items {
            item.validate()?;
            if !ids.insert(&item.id) {
                return Err("The provider returned duplicate recording IDs");
            }
        }
        Ok(())
    }
}

impl ProviderRecording {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        text(&self.title, 2048)?;
        text(&self.artist, 2048)?;
        optional(self.revision.as_deref(), 256)?;
        if self
            .duration_ms
            .is_some_and(|duration| duration > 9_007_199_254_740_991)
        {
            return Err("The provider returned an unsupported recording duration");
        }
        if self.disc_number == Some(0) || self.track_number == Some(0) {
            return Err("Provider track and disc numbers must be positive");
        }
        if let Some(release) = &self.release {
            text(&release.id, 256)?;
            text(&release.title, 2048)?;
            optional(release.artist.as_deref(), 2048)?;
        }
        Ok(())
    }
}

pub(crate) fn text(value: &str, maximum: usize) -> Result<(), &'static str> {
    if value.len() > maximum || value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err("Provider fields must use bounded nonempty text without control characters");
    }
    Ok(())
}
pub(crate) fn optional(value: Option<&str>, maximum: usize) -> Result<(), &'static str> {
    value
        .map(|value| text(value, maximum))
        .transpose()
        .map(|_| ())
}

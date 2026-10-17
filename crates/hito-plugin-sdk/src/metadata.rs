use crate::{
    MAX_SOURCES_PER_RECORDING, ProviderRecording,
    content::{optional, text},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const METADATA_PATH: &str = "v1/recordings/metadata";
pub const MAX_RECORDING_METADATA_BYTES: usize = 32_768;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RecordingMetadata {
    pub recording: ProviderRecording,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub musical_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ids: Option<Vec<String>>,
}

impl RecordingMetadata {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.recording.validate()?;
        optional(self.musical_title.as_deref(), 2048)?;
        optional(self.album_title.as_deref(), 2048)?;
        if let Some(sources) = &self.source_ids {
            if sources.len() > MAX_SOURCES_PER_RECORDING {
                return Err("The provider returned too many metadata source witnesses");
            }
            let mut ids = HashSet::new();
            for source in sources {
                text(source, 256)?;
                if !ids.insert(source) {
                    return Err("The provider returned duplicate metadata source witnesses");
                }
            }
        }
        let bytes =
            serde_json::to_vec(self).map_err(|_| "The provider metadata could not be encoded")?;
        if bytes.len() > MAX_RECORDING_METADATA_BYTES {
            return Err("The provider metadata exceeds the size limit");
        }
        Ok(())
    }
}

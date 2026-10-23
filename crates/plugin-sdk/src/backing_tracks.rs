use crate::{
    ProviderRecording, ProviderRecordingRef,
    encoded_size::{EncodedSizeError, validate_encoded_size},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const BACKING_TRACKS_CAPABILITIES_PATH: &str = "v1/backing-tracks/capabilities";
pub const BACKING_TRACKS_PATH: &str = "v1/backing-tracks";
pub const BACKING_TRACKS_VERSION: u32 = 1;
pub const MAX_BACKING_TRACKS_RESPONSE_BYTES: usize = 262_144;
const MAX_BACKING_RECORDINGS: usize = 16;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct BackingTracksCapabilities {
    pub backing_tracks_version: u32,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct BackingTracksRequest {
    pub recording: ProviderRecordingRef,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct BackingTracksResponse {
    pub recording: ProviderRecordingRef,
    pub items: Vec<ProviderRecording>,
}

impl BackingTracksCapabilities {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.backing_tracks_version != BACKING_TRACKS_VERSION {
            return Err("Unsupported backing tracks protocol version");
        }
        Ok(())
    }
}

impl BackingTracksRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.recording.validate()
    }
}

impl BackingTracksResponse {
    pub fn validate_for(&self, request: &BackingTracksRequest) -> Result<(), &'static str> {
        request.validate()?;
        self.recording.validate()?;
        if self.recording != request.recording {
            return Err("The provider returned backing tracks for a different recording version");
        }
        if self.items.len() > MAX_BACKING_RECORDINGS {
            return Err("The provider returned too many backing recordings");
        }
        let mut ids = HashSet::new();
        for recording in &self.items {
            recording.validate()?;
            if recording.id == self.recording.id {
                return Err(
                    "Backing tracks must use recording identities distinct from the original",
                );
            }
            if !ids.insert(&recording.id) {
                return Err("The provider returned duplicate backing recording IDs");
            }
        }
        validate_encoded_size(self, MAX_BACKING_TRACKS_RESPONSE_BYTES).map_err(
            |error| match error {
                EncodedSizeError::Exceeded => {
                    "The provider backing tracks response exceeds the size limit"
                }
                EncodedSizeError::Encoding => {
                    "The provider backing tracks response could not be encoded"
                }
            },
        )?;
        Ok(())
    }
}

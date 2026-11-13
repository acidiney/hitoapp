use serde::{Deserialize, Serialize};

pub const RECORDING_LOOKUP_PATH: &str = "v1/recordings/lookup";
pub const MAX_SOURCE_LINK_REFERENCE_BYTES: usize = 2048;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RecordingLookupRequest {
    pub reference: String,
}

impl RecordingLookupRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.reference.trim().is_empty()
            || self.reference.len() > MAX_SOURCE_LINK_REFERENCE_BYTES
            || self.reference.chars().any(char::is_control)
        {
            return Err(
                "Source links must use 1 to 2,048 bytes of nonblank text without control characters",
            );
        }
        Ok(())
    }
}

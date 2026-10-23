use crate::content::text;
use serde::{Deserialize, Serialize};

pub const ARTWORK_PATH: &str = "v1/artwork";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtworkEntity {
    Playlist,
    Recording,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ArtworkRequest {
    pub entity: ArtworkEntity,
    pub provider_id: String,
}

impl ArtworkRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.provider_id, 256)
    }
}

use super::CatalogReleaseGroup;
use crate::{
    content::text,
    encoded_size::{EncodedSizeError, validate_encoded_size},
};
use serde::{Deserialize, Serialize};

pub const CATALOG_RELEASE_GROUP_CAPABILITIES_PATH: &str = "v1/catalog/release-groups/capabilities";
pub const CATALOG_RELEASE_GROUP_PATH: &str = "v1/catalog/release-groups/detail";
pub const CATALOG_RELEASE_GROUP_VERSION: u32 = 1;
pub const MAX_CATALOG_RELEASE_GROUP_BYTES: usize = 32_768;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogReleaseGroupOperation {
    ReleaseGroup,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogReleaseGroupCapabilities {
    pub release_group_version: u32,
    pub operations: Vec<CatalogReleaseGroupOperation>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ReleaseGroupRequest {
    pub release_group_id: String,
}

impl CatalogReleaseGroupCapabilities {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.release_group_version != CATALOG_RELEASE_GROUP_VERSION {
            return Err("Unsupported catalog release-group protocol version");
        }
        if self.operations.len() != 1 {
            return Err("Declare exactly one catalog release-group operation");
        }
        Ok(())
    }
}

impl ReleaseGroupRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.release_group_id, 256)
    }
}

impl CatalogReleaseGroup {
    pub fn validate_for(&self, request: &ReleaseGroupRequest) -> Result<(), &'static str> {
        request.validate()?;
        self.validate()?;
        if self.id != request.release_group_id {
            return Err("The provider returned a different release group");
        }
        validate_encoded_size(self, MAX_CATALOG_RELEASE_GROUP_BYTES).map_err(|error| match error {
            EncodedSizeError::Exceeded => "The catalog release group exceeds the size limit",
            EncodedSizeError::Encoding => "The catalog release group could not be encoded",
        })
    }
}

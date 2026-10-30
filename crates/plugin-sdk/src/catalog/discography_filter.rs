use serde::{Deserialize, Serialize};

pub const CATALOG_DISCOGRAPHY_CAPABILITIES_PATH: &str = "v1/catalog/discography/capabilities";
pub const CATALOG_DISCOGRAPHY_VERSION: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogDiscographyOperation {
    ReleaseTypeFilter,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogDiscographyCapabilities {
    pub discography_version: u32,
    pub operations: Vec<CatalogDiscographyOperation>,
}

impl CatalogDiscographyCapabilities {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.discography_version != CATALOG_DISCOGRAPHY_VERSION {
            return Err("Unsupported catalog discography protocol version");
        }
        if self.operations.len() != 1 {
            return Err("Declare exactly one catalog discography operation");
        }
        Ok(())
    }
}

mod discography;
mod discovery;
mod edition;
mod edition_listing;
mod similarity;

pub use discography::{
    ArtistDiscographyPage, ArtistDiscographyRequest, CatalogCreditMember, CatalogReleaseGroup,
    CatalogReleaseGroupKind, MAX_CATALOG_CREDIT_MEMBERS, MAX_CATALOG_CURSOR_BYTES,
    MAX_CATALOG_PAGE_BYTES, MAX_CATALOG_PAGE_ITEMS,
};
pub use discovery::{
    ARTIST_SEARCH_PATH, ArtistSearchCandidate, ArtistSearchPage, ArtistSearchRequest,
    CATALOG_DISCOVERY_CAPABILITIES_PATH, CATALOG_DISCOVERY_VERSION, CatalogDiscoveryCapabilities,
    CatalogDiscoveryOperation, MAX_ARTIST_SEARCH_PAGE_BYTES, MAX_ARTIST_SEARCH_PAGE_ITEMS,
    MAX_ARTIST_SEARCH_QUERY_BYTES,
};
pub use edition::{
    CATALOG_EDITION_CAPABILITIES_PATH, CATALOG_EDITION_VERSION, CatalogEditionCapabilities,
    CatalogEditionMedium, CatalogEditionOperation, CatalogEditionTrackList, CatalogTrackKind,
    CatalogTrackOccurrence, EDITION_TRACK_LIST_PATH, EditionTrackListRequest,
    MAX_CATALOG_EDITION_BYTES, MAX_CATALOG_EDITION_MEDIA, MAX_CATALOG_EDITION_TRACKS,
};
pub use edition_listing::{
    CatalogEditionSummary, RELEASE_GROUP_EDITIONS_PATH, ReleaseGroupEditionsPage,
    ReleaseGroupEditionsRequest,
};

pub use similarity::{
    ARTIST_SIMILARITY_PATH, ArtistSimilarityPage, ArtistSimilarityRequest,
    CATALOG_SIMILARITY_CAPABILITIES_PATH, CATALOG_SIMILARITY_VERSION,
    CatalogSimilarityCapabilities, CatalogSimilarityIdentity, CatalogSimilarityOperation,
    MAX_SIMILAR_ARTISTS, MAX_SIMILAR_ARTISTS_BYTES, SimilarArtist,
};

use crate::content::{optional, text};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const CATALOG_CAPABILITIES_PATH: &str = "v1/catalog/capabilities";
pub const ARTIST_PROFILE_PATH: &str = "v1/catalog/artists/profile";
pub const ARTIST_DISCOGRAPHY_PATH: &str = "v1/catalog/artists/discography";
pub const CATALOG_VERSION: u32 = 1;
pub const MAX_CATALOG_PROFILE_BYTES: usize = 32_768;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogOperation {
    ArtistProfile,
    ArtistDiscography,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogCapabilities {
    pub catalog_version: u32,
    pub operations: Vec<CatalogOperation>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogArtistRef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogArtistKind {
    Person,
    Group,
    Orchestra,
    Choir,
    Character,
    Other,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ArtistProfileRequest {
    pub artist_id: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogArtistProfile {
    pub artist: CatalogArtistRef,
    pub sort_name: Option<String>,
    pub kind: Option<CatalogArtistKind>,
    pub disambiguation: Option<String>,
    pub revision: Option<String>,
}

impl CatalogCapabilities {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.catalog_version != CATALOG_VERSION {
            return Err("Unsupported catalog protocol version");
        }
        if self.operations.is_empty() || self.operations.len() > 2 {
            return Err("Declare between one and two catalog operations");
        }
        let mut operations = HashSet::new();
        for operation in &self.operations {
            if !operations.insert(operation) {
                return Err("The catalog descriptor repeated an operation");
            }
        }
        Ok(())
    }
}

impl CatalogArtistRef {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        text(&self.name, 2048)
    }
}

impl ArtistProfileRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.artist_id, 256)
    }
}

impl CatalogArtistProfile {
    pub fn validate_for(&self, request: &ArtistProfileRequest) -> Result<(), &'static str> {
        request.validate()?;
        self.artist.validate()?;
        if self.artist.id != request.artist_id {
            return Err("The provider returned a different artist profile");
        }
        optional(self.sort_name.as_deref(), 2048)?;
        optional(self.disambiguation.as_deref(), 2048)?;
        optional(self.revision.as_deref(), 256)?;
        let bytes = serde_json::to_vec(self)
            .map_err(|_| "The provider artist profile could not be encoded")?;
        if bytes.len() > MAX_CATALOG_PROFILE_BYTES {
            return Err("The provider artist profile exceeds the size limit");
        }
        Ok(())
    }
}

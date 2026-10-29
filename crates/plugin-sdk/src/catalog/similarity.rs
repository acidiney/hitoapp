use super::CatalogArtistRef;
use crate::{
    content::text,
    encoded_size::{EncodedSizeError, validate_encoded_size},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const CATALOG_SIMILARITY_CAPABILITIES_PATH: &str = "v1/catalog/similarity/capabilities";
pub const ARTIST_SIMILARITY_PATH: &str = "v1/catalog/similarity/artists";
pub const CATALOG_SIMILARITY_VERSION: u32 = 1;
pub const MAX_SIMILAR_ARTISTS: u16 = 25;
pub const MAX_SIMILAR_ARTISTS_BYTES: usize = 32_768;
const MAX_SCORE: f64 = 1.0e9;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogSimilarityOperation {
    ArtistSimilarity,
}

/// The identity namespace of the artists a similarity provider accepts and returns.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogSimilarityIdentity {
    MusicbrainzArtist,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogSimilarityCapabilities {
    pub similarity_version: u32,
    pub operations: Vec<CatalogSimilarityOperation>,
    pub artist_identity: CatalogSimilarityIdentity,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ArtistSimilarityRequest {
    pub artist_id: String,
    pub limit: u16,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct SimilarArtist {
    pub artist: CatalogArtistRef,
    /// Provider-relative closeness. It is only comparable within one response.
    pub score: Option<f64>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ArtistSimilarityPage {
    pub artist_id: String,
    pub items: Vec<SimilarArtist>,
}

impl CatalogSimilarityCapabilities {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.similarity_version != CATALOG_SIMILARITY_VERSION {
            return Err("Unsupported catalog similarity protocol version");
        }
        if self.operations.len() != 1 {
            return Err("Declare exactly one catalog similarity operation");
        }
        Ok(())
    }
}

impl ArtistSimilarityRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.artist_id, 256)?;
        if !(1..=MAX_SIMILAR_ARTISTS).contains(&self.limit) {
            return Err("Use a similar-artist limit between one and 25");
        }
        Ok(())
    }
}

impl SimilarArtist {
    fn validate(&self) -> Result<(), &'static str> {
        self.artist.validate()?;
        if self
            .score
            .is_some_and(|score| !score.is_finite() || !(0.0..=MAX_SCORE).contains(&score))
        {
            return Err("A similar-artist score must be a finite non-negative number");
        }
        Ok(())
    }
}

impl ArtistSimilarityPage {
    pub fn validate_for(&self, request: &ArtistSimilarityRequest) -> Result<(), &'static str> {
        request.validate()?;
        if self.artist_id != request.artist_id {
            return Err("The provider returned similar artists for a different artist");
        }
        if self.items.len() > usize::from(request.limit) {
            return Err("The provider returned too many similar artists");
        }
        let mut seen = HashSet::new();
        for item in &self.items {
            item.validate()?;
            if item.artist.id == request.artist_id {
                return Err("An artist cannot be similar to itself");
            }
            if !seen.insert(&item.artist.id) {
                return Err("The provider repeated a similar artist identity");
            }
        }
        validate_encoded_size(self, MAX_SIMILAR_ARTISTS_BYTES).map_err(|error| match error {
            EncodedSizeError::Exceeded => "The provider similar artists exceed the size limit",
            EncodedSizeError::Encoding => "The provider similar artists could not be encoded",
        })
    }
}

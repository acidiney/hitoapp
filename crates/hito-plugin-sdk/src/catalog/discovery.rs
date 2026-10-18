use super::{CatalogArtistKind, CatalogArtistRef, MAX_CATALOG_CURSOR_BYTES};
use crate::content::{optional, text};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io};

pub const CATALOG_DISCOVERY_CAPABILITIES_PATH: &str = "v1/catalog/discovery/capabilities";
pub const ARTIST_SEARCH_PATH: &str = "v1/catalog/artists/search";
pub const CATALOG_DISCOVERY_VERSION: u32 = 1;
pub const MAX_ARTIST_SEARCH_QUERY_BYTES: usize = 512;
pub const MAX_ARTIST_SEARCH_PAGE_ITEMS: u16 = 20;
pub const MAX_ARTIST_SEARCH_PAGE_BYTES: usize = 65_536;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogDiscoveryOperation {
    ArtistSearch,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogDiscoveryCapabilities {
    pub discovery_version: u32,
    pub operations: Vec<CatalogDiscoveryOperation>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ArtistSearchRequest {
    pub query: String,
    pub cursor: Option<String>,
    pub snapshot: Option<String>,
    pub limit: u16,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ArtistSearchCandidate {
    pub artist: CatalogArtistRef,
    pub kind: Option<CatalogArtistKind>,
    pub disambiguation: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ArtistSearchPage {
    pub query: String,
    pub snapshot: Option<String>,
    pub items: Vec<ArtistSearchCandidate>,
    pub total_count: Option<u32>,
    pub next_cursor: Option<String>,
}

impl CatalogDiscoveryCapabilities {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.discovery_version != CATALOG_DISCOVERY_VERSION {
            return Err("Unsupported catalog discovery protocol version");
        }
        if self.operations.len() != 1 {
            return Err("Declare the supported artist search discovery operation");
        }
        Ok(())
    }
}

impl ArtistSearchRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.query, MAX_ARTIST_SEARCH_QUERY_BYTES)?;
        optional(self.cursor.as_deref(), MAX_CATALOG_CURSOR_BYTES)?;
        optional(self.snapshot.as_deref(), 256)?;
        if !(1..=MAX_ARTIST_SEARCH_PAGE_ITEMS).contains(&self.limit) {
            return Err("Use an artist search limit between one and 20");
        }
        if self.cursor.is_none() && self.snapshot.is_some() {
            return Err("The first artist search cannot select a snapshot");
        }
        Ok(())
    }
}

impl ArtistSearchCandidate {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.artist.validate()?;
        optional(self.disambiguation.as_deref(), 2048)
    }
}

impl ArtistSearchPage {
    pub fn validate_for(&self, request: &ArtistSearchRequest) -> Result<(), &'static str> {
        request.validate()?;
        if self.query != request.query {
            return Err("The provider returned candidates for a different artist search");
        }
        optional(self.snapshot.as_deref(), 256)?;
        optional(self.next_cursor.as_deref(), MAX_CATALOG_CURSOR_BYTES)?;
        if request.cursor.is_some() && self.snapshot != request.snapshot {
            return Err("The artist search continuation changed its requested snapshot");
        }
        if self.items.len() > usize::from(request.limit) {
            return Err("The provider returned too many artist candidates");
        }
        if self
            .total_count
            .is_some_and(|count| (count as usize) < self.items.len())
        {
            return Err("The artist search total is smaller than its returned page");
        }
        if self.next_cursor.is_some()
            && (self.items.is_empty() || self.next_cursor == request.cursor)
        {
            return Err("The artist search continuation did not advance");
        }
        let mut artists = HashSet::new();
        for candidate in &self.items {
            candidate.validate()?;
            if !artists.insert(&candidate.artist.id) {
                return Err("The artist search page repeated an artist identity");
            }
        }
        let mut budget = EncodedPageBudget {
            remaining: MAX_ARTIST_SEARCH_PAGE_BYTES,
            exceeded: false,
        };
        serde_json::to_writer(&mut budget, self).map_err(|_| {
            if budget.exceeded {
                "The provider artist search page exceeds the size limit"
            } else {
                "The provider artist search page could not be encoded"
            }
        })?;
        Ok(())
    }
}

struct EncodedPageBudget {
    remaining: usize,
    exceeded: bool,
}

impl io::Write for EncodedPageBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            self.exceeded = true;
            return Err(io::ErrorKind::InvalidData.into());
        }
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

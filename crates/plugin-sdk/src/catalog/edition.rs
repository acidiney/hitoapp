use super::{CatalogCreditMember, MAX_CATALOG_CREDIT_MEMBERS};
use crate::{
    content::{optional, text},
    encoded_size::{EncodedSizeError, validate_encoded_size},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const CATALOG_EDITION_CAPABILITIES_PATH: &str = "v1/catalog/editions/capabilities";
pub const EDITION_TRACK_LIST_PATH: &str = "v1/catalog/editions/track-list";
pub const CATALOG_EDITION_VERSION: u32 = 1;
pub const MAX_CATALOG_EDITION_BYTES: usize = 262_144;
pub const MAX_CATALOG_EDITION_MEDIA: usize = 256;
pub const MAX_CATALOG_EDITION_TRACKS: usize = 4_096;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogEditionOperation {
    ReleaseGroupEditions,
    EditionTrackList,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogEditionCapabilities {
    pub edition_version: u32,
    pub operations: Vec<CatalogEditionOperation>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct EditionTrackListRequest {
    pub edition_id: String,
    pub release_group_id: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogEditionTrackList {
    pub id: String,
    pub release_group_id: String,
    pub title: String,
    pub artist_credit: Option<Vec<CatalogCreditMember>>,
    pub revision: Option<String>,
    pub media: Vec<CatalogEditionMedium>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogEditionMedium {
    pub position: u32,
    pub title: Option<String>,
    pub format: Option<String>,
    pub tracks: Vec<CatalogTrackOccurrence>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogTrackOccurrence {
    pub id: String,
    pub position: u32,
    pub number: Option<String>,
    pub kind: CatalogTrackKind,
    pub title: String,
    pub artist_credit: Option<Vec<CatalogCreditMember>>,
    pub duration_ms: Option<u64>,
    pub recording_id: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogTrackKind {
    Audio,
    Pregap,
    Data,
}

impl CatalogEditionCapabilities {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.edition_version != CATALOG_EDITION_VERSION {
            return Err("Unsupported catalog edition protocol version");
        }
        if self.operations.is_empty() || self.operations.len() > 2 {
            return Err("Declare between one and two catalog edition operations");
        }
        let mut operations = HashSet::new();
        for operation in &self.operations {
            if !operations.insert(operation) {
                return Err("The catalog edition descriptor repeated an operation");
            }
        }
        Ok(())
    }
}

impl EditionTrackListRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.edition_id, 256)?;
        text(&self.release_group_id, 256)
    }
}

impl CatalogEditionTrackList {
    pub fn validate_for(&self, request: &EditionTrackListRequest) -> Result<(), &'static str> {
        request.validate()?;
        text(&self.id, 256)?;
        text(&self.release_group_id, 256)?;
        if self.id != request.edition_id || self.release_group_id != request.release_group_id {
            return Err("The provider returned a different edition or release group");
        }
        text(&self.title, 2048)?;
        optional(self.revision.as_deref(), 256)?;
        validate_credit(self.artist_credit.as_deref())?;
        if self.media.is_empty() || self.media.len() > MAX_CATALOG_EDITION_MEDIA {
            return Err("The catalog edition has an unsupported number of media");
        }
        let mut total_tracks = 0;
        for medium in &self.media {
            if medium.tracks.len() > MAX_CATALOG_EDITION_TRACKS - total_tracks {
                return Err("The catalog edition has too many track occurrences");
            }
            total_tracks += medium.tracks.len();
        }
        let mut previous_medium = 0;
        let mut occurrences = HashSet::new();
        for medium in &self.media {
            if medium.position <= previous_medium {
                return Err("Catalog media must have increasing positive positions");
            }
            previous_medium = medium.position;
            optional(medium.title.as_deref(), 2048)?;
            optional(medium.format.as_deref(), 256)?;
            let mut previous_track = None;
            for track in &medium.tracks {
                track.validate()?;
                if previous_track.is_some_and(|position| track.position <= position) {
                    return Err("Catalog track occurrences must have increasing positions");
                }
                previous_track = Some(track.position);
                if !occurrences.insert(&track.id) {
                    return Err("The catalog edition repeated a track occurrence identity");
                }
            }
        }
        validate_encoded_size(self, MAX_CATALOG_EDITION_BYTES).map_err(|error| match error {
            EncodedSizeError::Exceeded => "The catalog edition exceeds the size limit",
            EncodedSizeError::Encoding => "The catalog edition could not be encoded",
        })
    }
}

impl CatalogTrackOccurrence {
    fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        text(&self.title, 2048)?;
        optional(self.number.as_deref(), 128)?;
        optional(self.recording_id.as_deref(), 256)?;
        if (self.kind == CatalogTrackKind::Pregap) != (self.position == 0) {
            return Err("Only a pregap occurrence can have position zero");
        }
        if self
            .duration_ms
            .is_some_and(|duration| duration > 9_007_199_254_740_991)
        {
            return Err("The catalog track occurrence has an unsupported duration");
        }
        validate_credit(self.artist_credit.as_deref())
    }
}

pub(super) fn validate_credit(credit: Option<&[CatalogCreditMember]>) -> Result<(), &'static str> {
    if let Some(credit) = credit {
        if credit.is_empty() || credit.len() > MAX_CATALOG_CREDIT_MEMBERS {
            return Err("Present artist credits must contain between one and 32 members");
        }
        for member in credit {
            member.validate()?;
        }
    }
    Ok(())
}

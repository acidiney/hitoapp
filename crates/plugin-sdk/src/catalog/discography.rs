use super::CatalogArtistRef;
use crate::content::{optional, text};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io};

pub const MAX_CATALOG_PAGE_ITEMS: u16 = 50;
pub const MAX_CATALOG_PAGE_BYTES: usize = 262_144;
pub const MAX_CATALOG_CURSOR_BYTES: usize = 2_048;
pub const MAX_CATALOG_CREDIT_MEMBERS: usize = 32;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogCreditMember {
    pub artist: Option<CatalogArtistRef>,
    pub credited_name: String,
    pub join_phrase: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogReleaseGroupKind {
    Album,
    Ep,
    Single,
    Broadcast,
    Other,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogReleaseGroup {
    pub id: String,
    pub title: String,
    pub artist_credit: Option<Vec<CatalogCreditMember>>,
    pub kind: Option<CatalogReleaseGroupKind>,
    pub secondary_types: Vec<String>,
    pub first_release_date: Option<String>,
    pub revision: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatalogReleaseTypeFilter {
    Album,
    Ep,
    Single,
    Other,
}

impl CatalogReleaseTypeFilter {
    pub fn admits(self, kind: Option<CatalogReleaseGroupKind>) -> bool {
        match self {
            Self::Album => kind == Some(CatalogReleaseGroupKind::Album),
            Self::Ep => kind == Some(CatalogReleaseGroupKind::Ep),
            Self::Single => kind == Some(CatalogReleaseGroupKind::Single),
            Self::Other => !matches!(
                kind,
                Some(
                    CatalogReleaseGroupKind::Album
                        | CatalogReleaseGroupKind::Ep
                        | CatalogReleaseGroupKind::Single
                )
            ),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ArtistDiscographyRequest {
    pub artist_id: String,
    pub cursor: Option<String>,
    pub snapshot: Option<String>,
    pub limit: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schema(required = false))]
    pub release_type: Option<CatalogReleaseTypeFilter>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ArtistDiscographyPage {
    pub artist_id: String,
    pub snapshot: Option<String>,
    pub items: Vec<CatalogReleaseGroup>,
    pub total_count: Option<u32>,
    pub next_cursor: Option<String>,
}

impl CatalogCreditMember {
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(artist) = &self.artist {
            artist.validate()?;
        }
        text(&self.credited_name, 2048)?;
        if self.join_phrase.len() > 128 || self.join_phrase.chars().any(char::is_control) {
            return Err(
                "Artist credit join phrases must be bounded text without control characters",
            );
        }
        Ok(())
    }
}

impl CatalogReleaseGroup {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        text(&self.title, 2048)?;
        optional(self.revision.as_deref(), 256)?;
        if let Some(credit) = &self.artist_credit {
            if credit.is_empty() || credit.len() > MAX_CATALOG_CREDIT_MEMBERS {
                return Err("Present artist credits must contain between one and 32 members");
            }
            for member in credit {
                member.validate()?;
            }
        }
        if self.secondary_types.len() > 16 {
            return Err("The catalog release group has too many secondary types");
        }
        let mut secondary_types = HashSet::new();
        for secondary_type in &self.secondary_types {
            text(secondary_type, 128)?;
            if !secondary_types.insert(secondary_type) {
                return Err("The catalog release group repeated a secondary type");
            }
        }
        if self
            .first_release_date
            .as_deref()
            .is_some_and(|date| !valid_partial_date(date))
        {
            return Err("The catalog release group has an invalid partial release date");
        }
        Ok(())
    }
}

impl ArtistDiscographyRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.artist_id, 256)?;
        optional(self.cursor.as_deref(), MAX_CATALOG_CURSOR_BYTES)?;
        optional(self.snapshot.as_deref(), 256)?;
        if !(1..=MAX_CATALOG_PAGE_ITEMS).contains(&self.limit) {
            return Err("Use a catalog page limit between one and 50");
        }
        if self.cursor.is_none() && self.snapshot.is_some() {
            return Err("The first catalog request cannot select a snapshot");
        }
        Ok(())
    }
}

impl ArtistDiscographyPage {
    pub fn validate_for(&self, request: &ArtistDiscographyRequest) -> Result<(), &'static str> {
        request.validate()?;
        text(&self.artist_id, 256)?;
        if self.artist_id != request.artist_id {
            return Err("The provider returned a different artist discography");
        }
        optional(self.snapshot.as_deref(), 256)?;
        optional(self.next_cursor.as_deref(), MAX_CATALOG_CURSOR_BYTES)?;
        if request.cursor.is_some() && self.snapshot != request.snapshot {
            return Err("The catalog continuation changed its requested snapshot");
        }
        if self.items.len() > usize::from(request.limit) {
            return Err("The provider returned too many catalog release groups");
        }
        if self
            .total_count
            .is_some_and(|count| (count as usize) < self.items.len())
        {
            return Err("The catalog total is smaller than its returned page");
        }
        if self.next_cursor.is_some()
            && (self.items.is_empty() || self.next_cursor == request.cursor)
        {
            return Err("The catalog continuation did not advance");
        }
        let mut groups = HashSet::new();
        for group in &self.items {
            group.validate()?;
            if request
                .release_type
                .is_some_and(|filter| !filter.admits(group.kind))
            {
                return Err("The catalog page held a release group of another type");
            }
            if !groups.insert(&group.id) {
                return Err("The catalog page repeated a release group identity");
            }
        }
        let mut budget = EncodedPageBudget {
            remaining: MAX_CATALOG_PAGE_BYTES,
            exceeded: false,
        };
        serde_json::to_writer(&mut budget, self).map_err(|_| {
            if budget.exceeded {
                "The provider discography page exceeds the size limit"
            } else {
                "The provider discography page could not be encoded"
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

pub(super) fn valid_partial_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    if !matches!(bytes.len(), 4 | 7 | 10)
        || bytes.iter().enumerate().any(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte != b'-'
            } else {
                !byte.is_ascii_digit()
            }
        })
    {
        return false;
    }
    let Ok(year) = date[..4].parse::<u16>() else {
        return false;
    };
    if year == 0 {
        return false;
    }
    if bytes.len() == 4 {
        return true;
    }
    let Ok(month) = date[5..7].parse::<usize>() else {
        return false;
    };
    if !(1..=12).contains(&month) {
        return false;
    }
    if bytes.len() == 7 {
        return true;
    }
    let Ok(day) = date[8..10].parse::<u8>() else {
        return false;
    };
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap_year { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    day > 0 && day <= days[month - 1]
}

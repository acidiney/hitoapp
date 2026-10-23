use super::{
    CatalogCreditMember, MAX_CATALOG_CURSOR_BYTES, MAX_CATALOG_PAGE_BYTES, MAX_CATALOG_PAGE_ITEMS,
    discography::valid_partial_date, edition::validate_credit,
};
use crate::{
    content::{optional, text},
    encoded_size::{EncodedSizeError, validate_encoded_size},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const RELEASE_GROUP_EDITIONS_PATH: &str = "v1/catalog/release-groups/editions";

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ReleaseGroupEditionsRequest {
    pub release_group_id: String,
    pub cursor: Option<String>,
    pub snapshot: Option<String>,
    pub limit: u16,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ReleaseGroupEditionsPage {
    pub release_group_id: String,
    pub snapshot: Option<String>,
    pub items: Vec<CatalogEditionSummary>,
    pub total_count: Option<u32>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct CatalogEditionSummary {
    pub id: String,
    pub title: String,
    pub artist_credit: Option<Vec<CatalogCreditMember>>,
    pub disambiguation: Option<String>,
    pub release_date: Option<String>,
    pub country: Option<String>,
    pub revision: Option<String>,
}

impl ReleaseGroupEditionsRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.release_group_id, 256)?;
        optional(self.cursor.as_deref(), MAX_CATALOG_CURSOR_BYTES)?;
        optional(self.snapshot.as_deref(), 256)?;
        if !(1..=MAX_CATALOG_PAGE_ITEMS).contains(&self.limit) {
            return Err("Use an edition page limit between one and 50");
        }
        if self.cursor.is_none() && self.snapshot.is_some() {
            return Err("The first edition request cannot select a snapshot");
        }
        Ok(())
    }
}

impl ReleaseGroupEditionsPage {
    pub fn validate_for(&self, request: &ReleaseGroupEditionsRequest) -> Result<(), &'static str> {
        request.validate()?;
        text(&self.release_group_id, 256)?;
        if self.release_group_id != request.release_group_id {
            return Err("The provider returned editions of a different release group");
        }
        optional(self.snapshot.as_deref(), 256)?;
        optional(self.next_cursor.as_deref(), MAX_CATALOG_CURSOR_BYTES)?;
        if request.cursor.is_some() && self.snapshot != request.snapshot {
            return Err("The edition continuation changed its requested snapshot");
        }
        if self.items.len() > usize::from(request.limit) {
            return Err("The provider returned too many catalog editions");
        }
        if self
            .total_count
            .is_some_and(|count| (count as usize) < self.items.len())
        {
            return Err("The edition total is smaller than its returned page");
        }
        if self.next_cursor.is_some()
            && (self.items.is_empty() || self.next_cursor == request.cursor)
        {
            return Err("The edition continuation did not advance");
        }
        let mut editions = HashSet::new();
        for edition in &self.items {
            edition.validate()?;
            if !editions.insert(&edition.id) {
                return Err("The catalog page repeated an edition identity");
            }
        }
        validate_encoded_size(self, MAX_CATALOG_PAGE_BYTES).map_err(|error| match error {
            EncodedSizeError::Exceeded => "The provider edition page exceeds the size limit",
            EncodedSizeError::Encoding => "The provider edition page could not be encoded",
        })
    }
}

impl CatalogEditionSummary {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        text(&self.title, 2048)?;
        validate_credit(self.artist_credit.as_deref())?;
        optional(self.disambiguation.as_deref(), 2048)?;
        optional(self.country.as_deref(), 128)?;
        optional(self.revision.as_deref(), 256)?;
        if self
            .release_date
            .as_deref()
            .is_some_and(|date| !valid_partial_date(date))
        {
            return Err("The catalog edition has an invalid partial release date");
        }
        Ok(())
    }
}

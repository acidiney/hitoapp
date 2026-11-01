use crate::{
    MAX_PLAYLIST_ITEMS,
    content::{optional, text},
    encoded_size::{EncodedSizeError, validate_encoded_size},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const FEATURED_PATH: &str = "v1/featured";
pub const MAX_FEATURED_ITEMS: u16 = 24;
pub const MAX_FEATURED_BYTES: usize = 32_768;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum FeaturedKind {
    Playlist,
    Album,
    NewRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct FeaturedRequest {
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct FeaturedItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub kind: FeaturedKind,
    pub reference: String,
    pub artwork_id: Option<String>,
    pub item_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct FeaturedPage {
    pub items: Vec<FeaturedItem>,
}

impl FeaturedRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=MAX_FEATURED_ITEMS).contains(&self.limit) {
            return Err("Use a featured limit between one and 24");
        }
        Ok(())
    }
}

impl FeaturedItem {
    fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        text(&self.title, 256)?;
        optional(self.subtitle.as_deref(), 256)?;
        text(&self.reference, 2048)?;
        optional(self.artwork_id.as_deref(), 256)?;
        if self
            .item_count
            .is_some_and(|count| count as usize > MAX_PLAYLIST_ITEMS)
        {
            return Err("A featured item exceeds the playlist item limit");
        }
        Ok(())
    }
}

impl FeaturedPage {
    pub fn validate_for(&self, request: &FeaturedRequest) -> Result<(), &'static str> {
        request.validate()?;
        if self.items.len() > usize::from(request.limit) {
            return Err("The provider returned too many featured items");
        }
        let mut seen = HashSet::new();
        for item in &self.items {
            item.validate()?;
            if !seen.insert(&item.id) {
                return Err("The provider repeated a featured item identity");
            }
        }
        validate_encoded_size(self, MAX_FEATURED_BYTES).map_err(|error| match error {
            EncodedSizeError::Exceeded => "The provider featured items exceed the size limit",
            EncodedSizeError::Encoding => "The provider featured items could not be encoded",
        })
    }
}

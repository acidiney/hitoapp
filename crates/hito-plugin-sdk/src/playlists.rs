use crate::{
    ProviderRecording,
    content::{optional, text},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const PLAYLIST_DETAIL_PATH: &str = "v1/playlists/detail";
pub const PLAYLIST_ITEMS_PATH: &str = "v1/playlists/items";
pub const MAX_PLAYLIST_ITEMS: usize = 10_000;
pub const MAX_PLAYLIST_PAGE_ITEMS: u16 = 100;

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaylistDetailRequest {
    Reference { reference: String },
    Canonical { playlist_id: String },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct PlaylistAttribution {
    pub label: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ProviderPlaylist {
    pub id: String,
    pub name: String,
    pub snapshot: String,
    pub total_count: u32,
    pub attribution: PlaylistAttribution,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct PlaylistItemsRequest {
    pub playlist_id: String,
    pub snapshot: String,
    pub cursor: Option<String>,
    pub limit: u16,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct PlaylistPage {
    pub playlist_id: String,
    pub snapshot: String,
    pub items: Vec<ProviderPlaylistEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ProviderPlaylistEntry {
    pub occurrence_id: Option<String>,
    pub item: ProviderPlaylistItem,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderPlaylistItem {
    Recording {
        recording: ProviderRecording,
    },
    Unavailable {
        provider_recording_id: Option<String>,
        title: Option<String>,
        artist: Option<String>,
    },
    Unknown {},
}

impl PlaylistDetailRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Reference { reference } => text(reference, 2048),
            Self::Canonical { playlist_id } => text(playlist_id, 256),
        }
    }
}

impl ProviderPlaylist {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.id, 256)?;
        text(&self.name, 256)?;
        text(&self.snapshot, 256)?;
        text(&self.attribution.label, 2048)?;
        if self.total_count as usize > MAX_PLAYLIST_ITEMS {
            return Err("The provider playlist exceeds the item limit");
        }
        Ok(())
    }
}

impl PlaylistItemsRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        text(&self.playlist_id, 256)?;
        text(&self.snapshot, 256)?;
        optional(self.cursor.as_deref(), 2048)?;
        if !(1..=MAX_PLAYLIST_PAGE_ITEMS).contains(&self.limit) {
            return Err("The playlist page limit must be between 1 and 100");
        }
        Ok(())
    }
}

impl PlaylistPage {
    pub fn validate(&self, requested: u16) -> Result<(), &'static str> {
        text(&self.playlist_id, 256)?;
        text(&self.snapshot, 256)?;
        optional(self.next_cursor.as_deref(), 2048)?;
        if self.items.len() > usize::from(requested.min(MAX_PLAYLIST_PAGE_ITEMS)) {
            return Err("The provider returned too many playlist items");
        }
        let mut occurrences = HashSet::new();
        for entry in &self.items {
            optional(entry.occurrence_id.as_deref(), 256)?;
            if entry
                .occurrence_id
                .as_ref()
                .is_some_and(|id| !occurrences.insert(id))
            {
                return Err("The provider repeated a playlist occurrence ID");
            }
            match &entry.item {
                ProviderPlaylistItem::Recording { recording } => recording.validate()?,
                ProviderPlaylistItem::Unavailable {
                    provider_recording_id,
                    title,
                    artist,
                } => {
                    optional(provider_recording_id.as_deref(), 256)?;
                    optional(title.as_deref(), 2048)?;
                    optional(artist.as_deref(), 2048)?;
                }
                ProviderPlaylistItem::Unknown {} => {}
            }
        }
        Ok(())
    }
}

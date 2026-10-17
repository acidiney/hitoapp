mod artwork;
mod authorization;
mod content;
mod playlists;
mod preparation;
pub use artwork::{ARTWORK_PATH, ArtworkEntity, ArtworkRequest};
pub use authorization::Authorization;
pub use content::{
    AUDIO_PATH, DETAIL_PATH, MAX_SOURCES_PER_RECORDING, ProviderRecording, ProviderRelease,
    ProviderSource, RecordingRequest, SEARCH_PATH, SOURCES_PATH, SearchPage, SearchRequest,
    SourcePage,
};
pub use playlists::{
    MAX_PLAYLIST_ITEMS, MAX_PLAYLIST_PAGE_ITEMS, PLAYLIST_DETAIL_PATH, PLAYLIST_ITEMS_PATH,
    PlaylistAttribution, PlaylistDetailRequest, PlaylistItemsRequest, PlaylistPage,
    ProviderPlaylist, ProviderPlaylistEntry, ProviderPlaylistItem,
};
pub use preparation::{
    MAX_PREPARATION_HANDLE_BYTES, PREPARATION_STATUS_PATH, PREPARE_SOURCE_PATH, PreparationReason,
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse,
};

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MANIFEST_PATH: &str = "v1/manifest";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub protocol_version: u32,
    pub capabilities: BTreeSet<Capability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization: Option<Authorization>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Search,
    Catalog,
    Metadata,
    Sources,
    SourcePreparation,
    Playlists,
    Artwork,
    Dashboard,
    Lyrics,
    BackingTracks,
}

impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err("Unsupported plugin protocol version".into());
        }
        if self.id.is_empty()
            || self.id.len() > 128
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
        {
            return Err("Plugin IDs must contain 1 to 128 ASCII letters, numbers, dots, hyphens, or underscores".into());
        }
        if self.name.trim().is_empty()
            || self.name.len() > 256
            || self.name.chars().any(char::is_control)
        {
            return Err(
                "Plugin names must contain 1 to 256 bytes without control characters".into(),
            );
        }
        if self.capabilities.is_empty() {
            return Err("Declare at least one capability".into());
        }
        if self.capabilities.contains(&Capability::SourcePreparation)
            && !self.capabilities.contains(&Capability::Sources)
        {
            return Err("Source preparation requires the sources capability".into());
        }
        if let Some(authorization) = &self.authorization {
            authorization.validate().map_err(str::to_owned)?;
        }
        Ok(())
    }
}

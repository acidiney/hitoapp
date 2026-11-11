mod artist_page;
mod artwork;
mod attribution;
mod authorization;
mod backing_tracks;
mod catalog;
mod content;
mod curated;
mod encoded_size;
mod feature_clause;
mod featured;
mod health;
mod isrc;
mod lyrics;
mod lyrics_v2;
mod metadata;
mod playlists;
mod preparation;
#[cfg(feature = "server")]
pub mod server;
pub use artist_page::{ArtistPageProvider, artist_page_provider};
pub use artwork::{ARTWORK_PATH, ArtworkEntity, ArtworkRequest};
pub use attribution::RecordingAttribution;
pub use authorization::Authorization;
pub use backing_tracks::{
    BACKING_TRACKS_CAPABILITIES_PATH, BACKING_TRACKS_PATH, BACKING_TRACKS_VERSION,
    BackingTracksCapabilities, BackingTracksRequest, BackingTracksResponse,
    MAX_BACKING_TRACKS_RESPONSE_BYTES,
};
pub use catalog::{
    ARTIST_DISCOGRAPHY_PATH, ARTIST_PROFILE_PATH, ARTIST_SEARCH_PATH, ARTIST_SIMILARITY_PATH,
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, ArtistSearchCandidate,
    ArtistSearchPage, ArtistSearchRequest, ArtistSimilarityPage, ArtistSimilarityRequest,
    CATALOG_CAPABILITIES_PATH, CATALOG_DISCOGRAPHY_CAPABILITIES_PATH, CATALOG_DISCOGRAPHY_VERSION,
    CATALOG_DISCOVERY_CAPABILITIES_PATH, CATALOG_DISCOVERY_VERSION,
    CATALOG_EDITION_CAPABILITIES_PATH, CATALOG_EDITION_VERSION,
    CATALOG_SIMILARITY_CAPABILITIES_PATH, CATALOG_SIMILARITY_VERSION, CATALOG_VERSION,
    CatalogArtistKind, CatalogArtistProfile, CatalogArtistRef, CatalogCapabilities,
    CatalogCreditMember, CatalogDiscographyCapabilities, CatalogDiscographyOperation,
    CatalogDiscoveryCapabilities, CatalogDiscoveryOperation, CatalogEditionCapabilities,
    CatalogEditionMedium, CatalogEditionOperation, CatalogEditionSummary, CatalogEditionTrackList,
    CatalogOperation, CatalogReleaseGroup, CatalogReleaseGroupKind, CatalogReleaseTypeFilter,
    CatalogSimilarityCapabilities, CatalogSimilarityIdentity, CatalogSimilarityOperation,
    CatalogTrackKind, CatalogTrackOccurrence, EDITION_TRACK_LIST_PATH, EditionTrackListRequest,
    MAX_ARTIST_SEARCH_PAGE_BYTES, MAX_ARTIST_SEARCH_PAGE_ITEMS, MAX_ARTIST_SEARCH_QUERY_BYTES,
    MAX_CATALOG_CREDIT_MEMBERS, MAX_CATALOG_CURSOR_BYTES, MAX_CATALOG_EDITION_BYTES,
    MAX_CATALOG_EDITION_MEDIA, MAX_CATALOG_EDITION_TRACKS, MAX_CATALOG_PAGE_BYTES,
    MAX_CATALOG_PAGE_ITEMS, MAX_CATALOG_PROFILE_BYTES, MAX_SIMILAR_ARTISTS,
    MAX_SIMILAR_ARTISTS_BYTES, RELEASE_GROUP_EDITIONS_PATH, ReleaseGroupEditionsPage,
    ReleaseGroupEditionsRequest, SimilarArtist,
};
pub use content::{
    AUDIO_PATH, CreditRole, CreditedArtist, DETAIL_PATH, MAX_SOURCES_PER_RECORDING,
    ProviderRecording, ProviderRecordingRef, ProviderRelease, ProviderSource, RecordingRequest,
    SEARCH_PATH, SOURCES_PATH, SearchPage, SearchRequest, SourcePage,
};
pub use curated::{
    CuratedCatalogError, CuratedEntryView, MAX_CURATED_CATALOG_BYTES, MAX_CURATED_ENTRIES,
    admit_curated_catalog,
};
pub use feature_clause::{bracketed_feature_names, credit_feature_names};
pub use featured::{
    FEATURED_PATH, FeaturedItem, FeaturedKind, FeaturedPage, FeaturedRequest, MAX_FEATURED_BYTES,
    MAX_FEATURED_ITEMS,
};
pub use health::{
    FailureTracker, HEALTH_PATH, HealthReason, HealthReport, HealthState, MAX_HEALTH_BYTES,
};
pub use isrc::normalize_isrc;
pub use lyrics::{
    LYRICS_CAPABILITIES_PATH, LYRICS_PATH, LYRICS_VERSION, LyricsCapabilities, LyricsRequest,
    LyricsResponse, MAX_LYRICS_RESPONSE_BYTES, TimedLyricLine, TimedLyrics,
};
pub use lyrics_v2::{
    AnchoredLyricLine, LYRICS_V2_CAPABILITIES_PATH, LYRICS_V2_PATH, LYRICS_V2_VERSION,
    LyricAttribution, LyricContent, LyricsCapabilitiesV2, LyricsDocument, LyricsResponseV2,
};
pub use metadata::{MAX_RECORDING_METADATA_BYTES, METADATA_PATH, RecordingMetadata};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_policy: Option<ContentPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ContentPolicy {
    pub source_persistence: SourcePersistence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SourcePersistence {
    ExplicitDownload,
    PlaybackOnly,
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
    Featured,
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
        if self.name.len() > 256
            || self.name.trim().is_empty()
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
        if self.capabilities.contains(&Capability::Featured)
            && !self.capabilities.contains(&Capability::Playlists)
        {
            return Err("Featured items need the playlists capability to open".into());
        }
        if let Some(authorization) = &self.authorization {
            authorization.validate().map_err(str::to_owned)?;
        }
        Ok(())
    }
}

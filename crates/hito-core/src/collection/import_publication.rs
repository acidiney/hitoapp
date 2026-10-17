use super::events::MAX_SYNC_PAYLOAD_BYTES;
use crate::{Error, Result};
use hito_plugin_sdk::{PlaylistAttribution, ProviderRecording};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CollectionPublication {
    PlaylistImported {
        publication_id: Uuid,
        snapshot: ImportedPlaylistSnapshot,
    },
    PlaylistFollowed {
        publication_id: Uuid,
        interval: super::PlaylistFollowInterval,
        snapshot: ImportedPlaylistSnapshot,
    },
    PlaylistFollowRefreshed {
        publication_id: Uuid,
        interval: super::PlaylistFollowInterval,
        snapshot: ImportedPlaylistSnapshot,
    },
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportedPlaylistSnapshot {
    pub playlist_id: Uuid,
    pub name: String,
    pub origin: ImportedPlaylistOrigin,
    pub recordings: Vec<ImportedPlaylistRecording>,
    pub entries: Vec<ImportedPlaylistEntry>,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportedPlaylistOrigin {
    pub installation_id: String,
    pub installation_revision: i64,
    pub service_id: String,
    pub service_revision: i64,
    pub provider: String,
    pub provider_playlist_id: String,
    pub snapshot: String,
    pub attribution: PlaylistAttribution,
    pub imported_at: i64,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportedPlaylistRecording {
    pub recording_id: Uuid,
    pub library_id: Uuid,
    pub release_id: Option<Uuid>,
    pub observation_id: Uuid,
    pub metadata: ProviderRecording,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportedPlaylistEntry {
    pub entry_id: Uuid,
    pub recording_id: Option<Uuid>,
    pub position: u32,
    pub origin: ImportedPlaylistEntryOrigin,
    pub title: Option<String>,
    pub artist: Option<String>,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportedPlaylistEntryOrigin {
    pub kind: ImportedPlaylistEntryKind,
    pub provider_recording_id: Option<String>,
    pub occurrence_id: Option<String>,
}

#[derive(Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportedPlaylistEntryKind {
    Recording,
    Unavailable,
    Unknown,
}

impl ImportedPlaylistEntryKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

pub(super) fn encode(publication: &CollectionPublication) -> Result<String> {
    let payload = serde_json::to_string(publication)
        .map_err(|_| Error::Internal("Collection publication could not be encoded".into()))?;
    if payload.len() > MAX_SYNC_PAYLOAD_BYTES {
        return Err(Error::Invalid(
            "The imported playlist exceeds the publication size limit".into(),
        ));
    }
    Ok(payload)
}

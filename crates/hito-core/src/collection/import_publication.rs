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
    pub recording_id: Uuid,
    pub position: u32,
    pub occurrence_id: Option<String>,
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

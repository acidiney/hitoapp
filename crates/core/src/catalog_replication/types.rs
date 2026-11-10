use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSnapshotMode {
    SnapshotOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogSnapshot {
    pub id: Uuid,
    pub account_id: String,
    pub library_id: String,
    pub mode: CatalogSnapshotMode,
    pub collection_watermark: i64,
    pub recording_count: usize,
    pub page_count: u64,
    pub expires_at: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogSnapshotPage {
    pub snapshot_id: Uuid,
    pub ordinal: u64,
    pub recordings: Vec<LocalRecordingFact>,
    pub next_ordinal: Option<u64>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalRecordingFact {
    pub id: String,
    pub library_id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: i64,
    pub sources: Vec<LocalSourceFact>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalSourceFact {
    pub id: String,
    pub observation_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub duration_ms: i64,
    pub server_relay: ServerRelayObservation,
    pub release: Option<LocalReleaseAppearance>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalReleaseAppearance {
    pub id: String,
    pub entry_id: String,
    pub title: String,
    pub artist: Option<String>,
    pub disc_number: Option<i64>,
    pub track_number: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServerRelayObservation {
    Ready,
    Unavailable,
}

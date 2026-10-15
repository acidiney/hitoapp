use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MetadataHistoryQuery {
    pub before: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingMetadata {
    pub recording_id: String,
    pub title: TitleMetadata,
    pub selected_observation: Option<MetadataObservation>,
    pub observations: ObservationPage,
}

#[derive(Serialize, ToSchema)]
pub struct TitleMetadata {
    pub value: String,
    pub inherited: String,
    pub personal: Option<String>,
    pub operation_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ObservationPage {
    pub items: Vec<MetadataObservation>,
    pub next_before: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct MetadataObservation {
    pub id: String,
    pub source_id: String,
    pub release_id: Option<String>,
    pub provider: String,
    pub origin: ObservationOrigin,
    pub observed_at: Option<i64>,
    pub inspection_version: i64,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub duration_ms: i64,
    pub disc_number: Option<i64>,
    pub track_number: Option<i64>,
    pub tags: Option<InspectedTags>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObservationOrigin {
    LegacyNormalized,
    FileInspection,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct InspectedTags {
    pub title: Option<ObservedTag>,
    pub artist: Option<ObservedTag>,
    pub album: Option<ObservedTag>,
    pub album_artist: Option<ObservedTag>,
    pub disc: Option<ObservedTag>,
    pub track: Option<ObservedTag>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ObservedTag {
    pub value: String,
    pub location: TagLocation,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TagLocation {
    Format,
    AudioStream { index: usize },
}

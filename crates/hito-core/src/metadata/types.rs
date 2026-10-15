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
    pub title: TextMetadata,
    pub artist: TextMetadata,
    #[schema(required = true)]
    pub selected_observation: Option<MetadataObservation>,
    pub observations: ObservationPage,
}

#[derive(Serialize, ToSchema)]
pub struct TextMetadata {
    pub value: String,
    pub inherited: String,
    #[schema(required = true)]
    pub personal: Option<String>,
    #[schema(required = true)]
    pub operation_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ObservationPage {
    pub items: Vec<MetadataObservation>,
    #[schema(required = true)]
    pub next_before: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct MetadataObservation {
    pub id: String,
    pub source_id: String,
    #[schema(required = true)]
    pub release_id: Option<String>,
    pub provider: String,
    pub origin: ObservationOrigin,
    #[schema(required = true)]
    pub observed_at: Option<i64>,
    pub inspection_version: i64,
    pub title: String,
    pub artist: String,
    #[schema(required = true)]
    pub album: Option<String>,
    #[schema(required = true)]
    pub album_artist: Option<String>,
    pub duration_ms: i64,
    #[schema(required = true)]
    pub disc_number: Option<i64>,
    #[schema(required = true)]
    pub track_number: Option<i64>,
    #[schema(required = true)]
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
    #[schema(required = true)]
    pub title: Option<ObservedTag>,
    #[schema(required = true)]
    pub artist: Option<ObservedTag>,
    #[schema(required = true)]
    pub album: Option<ObservedTag>,
    #[schema(required = true)]
    pub album_artist: Option<ObservedTag>,
    #[schema(required = true)]
    pub disc: Option<ObservedTag>,
    #[schema(required = true)]
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

use hito_plugin_sdk::{CreditRole, RecordingAttribution};
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
    /// The artists the provider credited for the shown text, in its order. Empty for a personal
    /// artist edit and for credits that were only text.
    pub credited_artists: Vec<CreditedArtistView>,
    pub observations: ObservationPage,
}

#[derive(Serialize, ToSchema)]
pub struct CreditedArtistView {
    pub name: String,
    pub role: CreditRole,
    #[schema(required = true)]
    pub join: Option<String>,
    /// The account's artist page for exactly this name, when one exists.
    #[schema(required = true)]
    pub artist_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ReleaseMetadata {
    pub release_id: String,
    pub title: TextMetadata,
    pub artist: OptionalTextMetadata,
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
pub struct OptionalTextMetadata {
    #[schema(required = true)]
    pub value: Option<String>,
    #[schema(required = true)]
    pub inherited: Option<String>,
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
    #[schema(required = true)]
    pub source_id: Option<String>,
    #[schema(required = true)]
    pub release_id: Option<String>,
    pub provider: String,
    pub origin: ObservationOrigin,
    #[schema(required = true)]
    pub observed_at: Option<i64>,
    #[schema(required = true)]
    pub inspection_version: Option<i64>,
    #[schema(required = true)]
    pub plugin_installation_id: Option<String>,
    #[schema(required = true)]
    pub provider_recording_id: Option<String>,
    #[schema(required = true)]
    pub provider_revision: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<RecordingAttribution>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObservationOrigin {
    LegacyNormalized,
    FileInspection,
    ProviderResponse,
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
    #[serde(default)]
    #[schema(required = true)]
    pub isrc: Option<ObservedTag>,
    #[serde(default)]
    #[schema(required = true)]
    pub genre: Option<ObservedTag>,
    #[serde(default)]
    #[schema(required = true)]
    pub musicbrainz_album_id: Option<ObservedTag>,
    #[serde(default)]
    #[schema(required = true)]
    pub musicbrainz_release_group_id: Option<ObservedTag>,
    #[serde(default)]
    #[schema(required = true)]
    pub musicbrainz_recording_id: Option<ObservedTag>,
    #[serde(default)]
    #[schema(required = true)]
    pub musicbrainz_release_track_id: Option<ObservedTag>,
    #[serde(default)]
    #[schema(required = true)]
    pub musicbrainz_artist_tags: Option<Vec<MusicBrainzArtistTag>>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct MusicBrainzArtistTag {
    pub role: MusicBrainzArtistRole,
    pub key: String,
    pub value: String,
    pub location: TagLocation,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicBrainzArtistRole {
    Artist,
    AlbumArtist,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ObservedTag {
    pub value: String,
    pub location: TagLocation,
}

#[derive(Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TagLocation {
    Format,
    AudioStream { index: usize },
}

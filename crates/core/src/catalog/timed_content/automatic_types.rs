use hito_plugin_sdk::{LyricsResponse, LyricsResponseV2, ProviderRecordingRef};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadAutomaticRecordingLyrics {}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AutomaticLyricsLookup {
    ReadyV1 {
        recording_id: String,
        installation_id: String,
        installation_revision: i64,
        label: String,
        provider_recording: ProviderRecordingRef,
        document_digest: String,
        document: Box<LyricsResponse>,
    },
    ReadyV2 {
        recording_id: String,
        installation_id: String,
        installation_revision: i64,
        label: String,
        provider_recording: ProviderRecordingRef,
        document_digest: String,
        document: Box<LyricsResponseV2>,
    },
    NoServices,
    InsufficientMetadata,
    NotFound,
    Ambiguous,
    Unsupported,
    Unavailable,
    Cancelled,
}

impl AutomaticLyricsLookup {
    pub(super) fn outcome(&self) -> &'static str {
        match self {
            Self::ReadyV1 { .. } | Self::ReadyV2 { .. } => "ready",
            Self::NoServices => "no_services",
            Self::InsufficientMetadata => "insufficient_metadata",
            Self::NotFound => "not_found",
            Self::Ambiguous => "ambiguous",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

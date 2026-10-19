use hito_plugin_sdk::{LyricsResponse, ProviderRecordingRef};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadRecordingLyrics {
    pub observation_id: String,
    pub installation_revision: i64,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingLyricsProviders {
    pub recording_id: String,
    pub items: Vec<RecordingLyricsProvider>,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingLyricsProvider {
    pub observation_id: String,
    pub installation_id: String,
    pub installation_revision: i64,
    pub provider_recording: ProviderRecordingRef,
    pub label: String,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LyricsLookup {
    Ready {
        recording_id: String,
        observation_id: String,
        document: Box<LyricsResponse>,
    },
    Unsupported,
    Unavailable,
    Cancelled,
}

impl LyricsLookup {
    pub(crate) fn outcome(&self) -> &'static str {
        match self {
            Self::Ready { document, .. } if document.lyrics.is_none() => "not_found",
            Self::Ready { .. } => "ready",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

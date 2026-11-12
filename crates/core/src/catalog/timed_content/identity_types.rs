use crate::ProviderCandidate;
use hito_plugin_sdk::{LyricsResponse, LyricsResponseV2, ProviderRecordingRef, RecordingMetadata};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct RecordingLyricServices {
    pub recording_id: String,
    pub items: Vec<RecordingLyricService>,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingLyricService {
    pub installation_id: String,
    pub installation_revision: i64,
    pub label: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchRecordingLyrics {
    pub installation_id: String,
    pub installation_revision: i64,
    pub query: String,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingLyricReview {
    #[schema(inline)]
    pub candidate: RecordingMetadata,
    pub review_stamp: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RetainRecordingLyricIdentity {
    pub candidate: ProviderCandidate,
    pub review_stamp: String,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordingLyricSelector {
    Observation {
        observation_id: String,
    },
    Reviewed {
        identity_id: String,
        identity_revision: i64,
    },
}

#[derive(Serialize, ToSchema)]
pub struct RecordingLyricIdentity {
    pub selectable: bool,
    pub selector: RecordingLyricSelector,
    pub installation_id: String,
    pub installation_revision: i64,
    pub provider_recording: ProviderRecordingRef,
    pub label: String,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingLyricIdentities {
    pub recording_id: String,
    pub items: Vec<RecordingLyricIdentity>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadSelectedRecordingLyrics {
    pub selector: RecordingLyricSelector,
    pub installation_revision: i64,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SelectedLyricsLookup {
    ReadyV1 {
        recording_id: String,
        selector: RecordingLyricSelector,
        provider_recording: ProviderRecordingRef,
        document_digest: String,
        document: Box<LyricsResponse>,
    },
    ReadyV2 {
        recording_id: String,
        selector: RecordingLyricSelector,
        provider_recording: ProviderRecordingRef,
        document_digest: String,
        document: Box<LyricsResponseV2>,
    },
    Unsupported,
    Unavailable,
    Cancelled,
}

impl SelectedLyricsLookup {
    pub(super) fn outcome(&self) -> &'static str {
        match self {
            Self::ReadyV1 { document, .. } if document.lyrics.is_none() => "not_found",
            Self::ReadyV2 { document, .. } if document.lyrics.is_none() => "not_found",
            Self::ReadyV1 { .. } | Self::ReadyV2 { .. } => "ready",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

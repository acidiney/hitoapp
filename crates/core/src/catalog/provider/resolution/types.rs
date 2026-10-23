use crate::{Error, Result, Source};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveSource {
    pub request_id: Uuid,
}

#[derive(Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceResolutionOutcome {
    Ready,
    NoSuitableSource,
    ProviderUnavailable,
    ProtectedChoice,
    Cancelled,
}

impl SourceResolutionOutcome {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NoSuitableSource => "no_suitable_source",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ProtectedChoice => "protected_choice",
            Self::Cancelled => "cancelled",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "ready" => Ok(Self::Ready),
            "no_suitable_source" => Ok(Self::NoSuitableSource),
            "provider_unavailable" => Ok(Self::ProviderUnavailable),
            "protected_choice" => Ok(Self::ProtectedChoice),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(Error::Internal(
                "Stored source resolution outcome is invalid".into(),
            )),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct SourceResolution {
    pub request_id: Uuid,
    pub resolution_id: Uuid,
    pub recording_id: String,
    pub outcome: SourceResolutionOutcome,
    pub sources: Vec<Source>,
}

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::Error;

pub(crate) const MAX_CANDIDATES: usize = 32;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Provider {
    Youtube,
    Jamendo,
}

impl Provider {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Youtube => "youtube",
            Self::Jamendo => "jamendo",
        }
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq, Hash, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceRef {
    pub(crate) provider: Provider,
    #[doc = "YouTube requires exactly 11 ASCII letters, digits, underscores, or hyphens. Jamendo requires a positive canonical decimal u64 without leading zeros."]
    #[schema(min_length = 1, max_length = 20)]
    pub(crate) recording_id: String,
}

impl SourceRef {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let bytes = self.recording_id.as_bytes();
        let valid = match self.provider {
            Provider::Youtube => {
                bytes.len() == 11
                    && bytes
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-'))
            }
            Provider::Jamendo => {
                (1..=20).contains(&bytes.len())
                    && bytes[0] != b'0'
                    && bytes.iter().all(u8::is_ascii_digit)
                    && self.recording_id.parse::<u64>().is_ok()
            }
        };
        valid.then_some(()).ok_or(Error::InvalidInput)
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProvisionReporter {}

#[derive(Serialize, ToSchema)]
pub(crate) struct ReporterCredential {
    pub(crate) reporter_id: Uuid,
    #[schema(min_length = 64, max_length = 64, pattern = "^[0-9a-f]{64}$")]
    pub(crate) token: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeKind {
    SuccessfulPlayback,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmitOutcome {
    #[doc = "Version-4 UUID. Accepted UUID text forms normalize to the same submission identifier."]
    pub(crate) submission_id: Uuid,
    pub(crate) source: SourceRef,
    pub(crate) outcome: OutcomeKind,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct OutcomeReceipt {
    pub(crate) receipt_id: Uuid,
    pub(crate) submission_id: Uuid,
    pub(crate) status: &'static str,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RankCandidates {
    #[schema(schema_with = candidate_sources_schema)]
    pub(crate) candidates: Vec<SourceRef>,
}

fn candidate_sources_schema() -> utoipa::openapi::schema::Array {
    utoipa::openapi::schema::ArrayBuilder::new()
        .items(utoipa::openapi::Ref::from_schema_name(SourceRef::name()))
        .min_items(Some(1))
        .max_items(Some(MAX_CANDIDATES))
        .unique_items(true)
        .build()
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RankedSource {
    pub(crate) source: SourceRef,
    pub(crate) successful_reporters: i64,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct Ranking {
    pub(crate) ranked: Vec<RankedSource>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct Rejection {
    pub(crate) error: &'static str,
}

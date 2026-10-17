use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Error;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
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

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceRef {
    pub(crate) provider: Provider,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProvisionReporter {}

#[derive(Serialize)]
pub(crate) struct ReporterCredential {
    pub(crate) reporter_id: Uuid,
    pub(crate) token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeKind {
    SuccessfulPlayback,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmitOutcome {
    pub(crate) submission_id: Uuid,
    pub(crate) source: SourceRef,
    pub(crate) outcome: OutcomeKind,
}

#[derive(Serialize)]
pub(crate) struct OutcomeReceipt {
    pub(crate) receipt_id: Uuid,
    pub(crate) submission_id: Uuid,
    pub(crate) status: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RankCandidates {
    pub(crate) candidates: Vec<SourceRef>,
}

#[derive(Serialize)]
pub(crate) struct RankedSource {
    pub(crate) source: SourceRef,
    pub(crate) successful_reporters: i64,
}

#[derive(Serialize)]
pub(crate) struct Ranking {
    pub(crate) ranked: Vec<RankedSource>,
}

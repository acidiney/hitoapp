use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::wire::SourceRef;

pub(crate) const MAX_EXPECTED_REVISION: i64 = 9_007_199_254_740_990;

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Relation {
    SameRecording,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CorrectionStatus {
    #[default]
    Pending,
    Approved,
    Rejected,
}

impl CorrectionStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SuggestCorrection {
    pub(crate) submission_id: Uuid,
    pub(crate) left: SourceRef,
    pub(crate) right: SourceRef,
    pub(crate) relation: Relation,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct CorrectionReceipt {
    pub(crate) correction_id: Uuid,
    pub(crate) submission_id: Uuid,
    pub(crate) status: &'static str,
}

#[derive(Default, Deserialize, IntoParams)]
#[serde(default, deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListCorrections {
    pub(crate) status: CorrectionStatus,
    #[param(minimum = 0)]
    pub(crate) after: i64,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct CorrectionSummary {
    pub(crate) correction_id: Uuid,
    pub(crate) left: SourceRef,
    pub(crate) right: SourceRef,
    pub(crate) relation: Relation,
    pub(crate) status: CorrectionStatus,
    #[schema(minimum = 1, maximum = 9_007_199_254_740_991_i64)]
    pub(crate) revision: i64,
    pub(crate) created_at: i64,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct CorrectionPage {
    #[schema(max_items = 32)]
    pub(crate) suggestions: Vec<CorrectionSummary>,
    pub(crate) next_after: Option<i64>,
}

#[derive(Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewDecision {
    Approve,
    Reject,
}

impl ReviewDecision {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Reject => "reject",
        }
    }

    pub(crate) fn status(self) -> CorrectionStatus {
        match self {
            Self::Approve => CorrectionStatus::Approved,
            Self::Reject => CorrectionStatus::Rejected,
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewCorrection {
    pub(crate) request_id: Uuid,
    #[schema(minimum = 1, maximum = 9_007_199_254_740_990_i64)]
    pub(crate) expected_revision: i64,
    pub(crate) decision: ReviewDecision,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ReviewReceipt {
    pub(crate) request_id: Uuid,
    pub(crate) correction_id: Uuid,
    #[schema(minimum = 2, maximum = 9_007_199_254_740_991_i64)]
    pub(crate) revision: i64,
    pub(crate) decision: ReviewDecision,
    pub(crate) status: CorrectionStatus,
    pub(crate) validation_method: &'static str,
}

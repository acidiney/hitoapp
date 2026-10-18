use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, ToSchema, sqlx::FromRow)]
pub struct CommunityPreferences {
    pub revision: i64,
    pub playback_reporting: bool,
    pub metadata_corrections: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCommunityPreferences {
    pub revision: i64,
    pub playback_reporting: bool,
    pub metadata_corrections: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SuccessfulPlayback {
    pub submission_id: uuid::Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommunityReportAdmission {
    Ignored,
    Queued,
    AlreadyAccepted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct CommunityCredentialReset {
    pub reporters_removed: u32,
    pub reports_cancelled: u32,
}

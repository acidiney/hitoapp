use crate::{Error, Result};
use hito_plugin_sdk::PreparationReason;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PrepareSource {
    pub request_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct SourcePreparationReceipt {
    pub request_id: Uuid,
    pub preparation_id: Uuid,
    pub recording_id: String,
    pub source_id: String,
}

#[derive(Serialize, ToSchema)]
pub struct SourcePreparationStatus {
    pub preparation_id: Uuid,
    pub recording_id: String,
    pub source_id: String,
    pub phase: SourcePreparationPhase,
    pub progress_basis_points: Option<u16>,
    pub eta_seconds: Option<u32>,
    pub reason: Option<PreparationReason>,
    pub next_check_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourcePreparationPhase {
    Queued,
    Submitting,
    Preparing,
    Ready,
    Blocked,
    Failed,
    SubmissionUnknown,
    Cancelled,
}

#[derive(sqlx::FromRow)]
pub(super) struct StatusRow {
    pub id: String,
    pub recording_id: String,
    pub source_id: String,
    pub phase: String,
    pub progress_basis_points: Option<i64>,
    pub eta_seconds: Option<i64>,
    pub reason: Option<String>,
    pub next_check_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl StatusRow {
    pub(super) fn into_status(self) -> Result<SourcePreparationStatus> {
        let invalid = || Error::Internal("Stored source preparation status is invalid".into());
        let phase =
            serde_json::from_value(serde_json::Value::String(self.phase)).map_err(|_| invalid())?;
        let reason = self
            .reason
            .map(|reason| serde_json::from_value(serde_json::Value::String(reason)))
            .transpose()
            .map_err(|_| invalid())?;
        Ok(SourcePreparationStatus {
            preparation_id: identity(&self.id)?,
            recording_id: self.recording_id,
            source_id: self.source_id,
            phase,
            progress_basis_points: self
                .progress_basis_points
                .map(u16::try_from)
                .transpose()
                .map_err(|_| invalid())?,
            eta_seconds: self
                .eta_seconds
                .map(u32::try_from)
                .transpose()
                .map_err(|_| invalid())?,
            reason,
            next_check_at: self.next_check_at,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

pub(super) fn identity(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|_| Error::Internal("Stored source preparation identity is invalid".into()))
}

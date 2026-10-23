use crate::content::text;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PREPARE_SOURCE_PATH: &str = "v1/sources/prepare";
pub const PREPARATION_STATUS_PATH: &str = "v1/sources/preparation/status";
pub const MAX_PREPARATION_HANDLE_BYTES: usize = 2048;

#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct PreparationRequest {
    pub operation_id: Uuid,
    pub recording_id: String,
    pub source_id: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct PreparationStatusRequest {
    pub operation_id: Uuid,
    pub recording_id: String,
    pub source_id: String,
    pub handle: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct PreparationSubmitResponse {
    pub operation_id: Uuid,
    pub recording_id: String,
    pub source_id: String,
    pub submission: PreparationSubmission,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreparationSubmission {
    Accepted {
        handle: String,
        status: PreparationStatus,
    },
    NotSubmitted {
        reason: PreparationReason,
        retry_after_seconds: Option<u32>,
    },
    SubmissionUnknown {},
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct PreparationStatusResponse {
    pub operation_id: Uuid,
    pub recording_id: String,
    pub source_id: String,
    pub handle: String,
    pub status: PreparationStatus,
}

#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreparationStatus {
    Queued {
        poll_after_seconds: u32,
    },
    Preparing {
        progress_basis_points: Option<u16>,
        eta_seconds: Option<u32>,
        poll_after_seconds: u32,
    },
    Ready {},
    Blocked {
        reason: PreparationReason,
        retry_after_seconds: Option<u32>,
    },
    Failed {
        reason: PreparationReason,
    },
    SubmissionUnknown {},
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PreparationReason {
    AuthorizationRequired,
    QuotaReached,
    RateLimited,
    ProviderRestricted,
    SourceUnavailable,
    SourceChanged,
    Unsupported,
    ProviderUnavailable,
    InvalidResponse,
}

impl PreparationRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        identity(self.operation_id, &self.recording_id, &self.source_id)
    }
}

impl PreparationStatusRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        identity(self.operation_id, &self.recording_id, &self.source_id)?;
        text(&self.handle, MAX_PREPARATION_HANDLE_BYTES)
    }
}

impl PreparationSubmitResponse {
    pub fn validate(&self, request: &PreparationRequest) -> Result<(), &'static str> {
        same_identity(
            self.operation_id,
            &self.recording_id,
            &self.source_id,
            request.operation_id,
            &request.recording_id,
            &request.source_id,
        )?;
        match &self.submission {
            PreparationSubmission::Accepted { handle, status } => {
                text(handle, MAX_PREPARATION_HANDLE_BYTES)?;
                status.validate()
            }
            PreparationSubmission::NotSubmitted {
                retry_after_seconds,
                ..
            } => optional_delay(*retry_after_seconds),
            PreparationSubmission::SubmissionUnknown {} => Ok(()),
        }
    }
}

impl PreparationStatusResponse {
    pub fn validate(&self, request: &PreparationStatusRequest) -> Result<(), &'static str> {
        same_identity(
            self.operation_id,
            &self.recording_id,
            &self.source_id,
            request.operation_id,
            &request.recording_id,
            &request.source_id,
        )?;
        if self.handle != request.handle {
            return Err("The provider returned a different preparation handle");
        }
        text(&self.handle, MAX_PREPARATION_HANDLE_BYTES)?;
        self.status.validate()
    }
}

impl PreparationStatus {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Queued { poll_after_seconds } => delay(*poll_after_seconds),
            Self::Preparing {
                progress_basis_points,
                eta_seconds,
                poll_after_seconds,
            } => {
                if progress_basis_points.is_some_and(|progress| progress > 10_000)
                    || eta_seconds.is_some_and(|seconds| seconds > 7 * 24 * 60 * 60)
                {
                    return Err("The provider returned invalid preparation progress");
                }
                delay(*poll_after_seconds)
            }
            Self::Blocked {
                retry_after_seconds,
                ..
            } => optional_delay(*retry_after_seconds),
            Self::Ready {} | Self::Failed { .. } | Self::SubmissionUnknown {} => Ok(()),
        }
    }
}

fn identity(operation: Uuid, recording: &str, source: &str) -> Result<(), &'static str> {
    if operation.is_nil() {
        return Err("Use a nonempty preparation operation identity");
    }
    text(recording, 256)?;
    text(source, 256)
}

fn same_identity(
    operation: Uuid,
    recording: &str,
    source: &str,
    expected_operation: Uuid,
    expected_recording: &str,
    expected_source: &str,
) -> Result<(), &'static str> {
    identity(operation, recording, source)?;
    if operation != expected_operation
        || recording != expected_recording
        || source != expected_source
    {
        return Err("The provider returned a different preparation target");
    }
    Ok(())
}

fn optional_delay(seconds: Option<u32>) -> Result<(), &'static str> {
    seconds.map(delay).transpose().map(|_| ())
}

fn delay(seconds: u32) -> Result<(), &'static str> {
    if (5..=24 * 60 * 60).contains(&seconds) {
        Ok(())
    } else {
        Err("Preparation polling must wait between five seconds and one day")
    }
}

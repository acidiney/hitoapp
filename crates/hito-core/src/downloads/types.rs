use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DownloadRequest {
    pub request_id: Uuid,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DownloadReceipt {
    pub request_id: Uuid,
    pub intent_id: String,
    pub recording_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Queued,
    Running,
    Verifying,
    Ready,
    Failed,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DownloadStatus {
    pub status: DownloadState,
    pub intent_id: String,
    pub recording_id: String,
    pub source_id: Option<String>,
    pub host_device_id: String,
    pub bytes_received: u64,
    pub total_bytes: Option<u64>,
    pub error: Option<String>,
}

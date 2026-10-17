use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistFollowInterval {
    Hourly,
    Daily,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistFollowStatus {
    Ready,
    Queued,
    Running,
    Failed,
    Paused,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FollowProviderPlaylist {
    pub request_id: Uuid,
    pub installation_revision: i64,
    pub playlist_reference: String,
    pub interval: PlaylistFollowInterval,
}

#[derive(Serialize, ToSchema)]
pub struct PlaylistFollowReceipt {
    pub request_id: Uuid,
    pub playlist_id: Uuid,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshFollowedPlaylist {
    pub request_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct PlaylistFollowRefresh {
    pub request_id: Uuid,
    pub job_id: Uuid,
    pub playlist_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct PlaylistFollow {
    pub playlist_id: Uuid,
    pub generation: i64,
    pub interval: PlaylistFollowInterval,
    pub snapshot: String,
    pub last_refreshed_at: i64,
    pub next_refresh_at: i64,
    pub status: PlaylistFollowStatus,
    pub error: Option<String>,
}

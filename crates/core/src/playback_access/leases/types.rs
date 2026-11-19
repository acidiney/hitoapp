use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BeginPlaybackLease {
    pub recording_id: String,
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(min_length = 1, max_length = 64)]
    pub client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, ToSchema, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ReleaseClientLeases {
    #[param(min_length = 1, max_length = 64)]
    pub client: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaybackLease {
    pub id: Uuid,
    pub expires_at: i64,
}

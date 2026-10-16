use hito_playback::PlayerSnapshot;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct PlayerStatus {
    pub native: bool,
    pub epoch: Option<Uuid>,
    pub state: Option<PlayerSnapshot>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayerClaim {
    pub epoch: Uuid,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayerQueueRequest {
    pub lease: Uuid,
    pub revision: u64,
    pub offset: usize,
    pub limit: usize,
}

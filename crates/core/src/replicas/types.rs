use crate::{Actor, Credentials, Error, Result, User};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct SyncClientId(String);

impl SyncClientId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err(Error::Invalid("Use a synchronization client ID of 1 to 128 letters, numbers, hyphens, or underscores".into()));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectReplica {
    pub endpoint: String,
    pub credentials: Credentials,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaConnectionState {
    Initializing,
    Ready,
    Disconnected,
}

#[derive(Serialize, ToSchema)]
pub struct ReplicaConnection {
    pub id: Uuid,
    pub account: User,
    pub client_id: SyncClientId,
    pub state: ReplicaConnectionState,
}

#[derive(Clone)]
pub struct ReplicaSession {
    pub(super) owner: Actor,
    pub(super) instance: Uuid,
    pub(super) signal: std::sync::Arc<super::lifecycle::LeaseSignal>,
    pub(super) connection: Uuid,
    pub(super) remote_account: String,
    pub(super) client_id: SyncClientId,
    pub(super) connection_generation: i64,
    pub(super) activation: Uuid,
}

impl std::fmt::Debug for ReplicaSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplicaSession")
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize, ToSchema)]
pub struct QueuedReplicaChange {
    pub operation_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct ReplicaSyncOutcome {
    pub pushed: usize,
    pub received: usize,
    pub cursor: i64,
    pub pending: usize,
    pub has_more: bool,
}

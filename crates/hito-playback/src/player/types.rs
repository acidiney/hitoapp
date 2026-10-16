use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlayerError {
    #[error("Sign in to continue")]
    Unauthorized,
    #[error("Playback ownership or ordering changed; inspect the current state")]
    Conflict,
    #[error("Playback admission is busy")]
    Busy,
    #[error("The playback owner has closed")]
    Closed,
    #[error("Playback authority could not be checked")]
    Unavailable,
    #[error("The requested recording is unavailable to this account")]
    NotFound,
    #[error("The playback request is invalid or exceeds its limits")]
    Invalid,
    #[error("Native audio output is not enabled")]
    Unsupported,
    #[error("Audio could not be played")]
    Playback,
    #[error("Listening progress could not be saved")]
    Persistence,
}
impl From<hito_core::Error> for PlayerError {
    fn from(error: hito_core::Error) -> Self {
        tracing::debug!(
            event = "player_authority_failed",
            error_kind = error.code(),
            "Playback authority check failed"
        );
        match error {
            hito_core::Error::Unauthorized => Self::Unauthorized,
            hito_core::Error::NotFound | hito_core::Error::Forbidden => Self::NotFound,
            hito_core::Error::Invalid(_) => Self::Invalid,
            _ => Self::Unavailable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerSnapshot {
    pub revision: u64,
    pub selection: u64,
    pub queue_len: usize,
    pub current: Option<PlayerEntryRef>,
    pub source_id: Option<String>,
    pub desired_playing: bool,
    pub playing: bool,
    pub seeking: bool,
    pub volume: u16,
    pub phase: PlayerPhase,
    pub error: Option<PlayerError>,
    pub last_failure: Option<PlayerFailure>,
    pub persistence_error: Option<PlayerError>,
}
impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            selection: 0,
            queue_len: 0,
            current: None,
            source_id: None,
            desired_playing: false,
            playing: false,
            seeking: false,
            volume: 1000,
            phase: PlayerPhase::Idle,
            error: None,
            last_failure: None,
            persistence_error: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerFailure {
    pub entry: PlayerEntryRef,
    pub error: PlayerError,
}
#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerInspection {
    pub native: bool,
    pub epoch: Uuid,
    pub state: Option<PlayerSnapshot>,
}
#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerAttachment {
    pub native: bool,
    pub epoch: Uuid,
    pub lease: Uuid,
    pub state: PlayerSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerEntryRef {
    pub id: Uuid,
    pub recording_id: Uuid,
}
#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerQueueEntry {
    pub id: Uuid,
    pub recording: hito_core::Recording,
}
#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerQueuePage {
    pub revision: u64,
    pub total: usize,
    pub offset: usize,
    pub entries: Vec<PlayerQueueEntry>,
}
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct PlayerCommand {
    pub id: Uuid,
    pub lease: Uuid,
    pub sequence: u64,
    pub revision: u64,
    pub action: PlayerAction,
}
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlayerAction {
    Resume {},
    Pause {},
    Seek { position_ms: u64 },
    Volume { level: u16 },
    Next {},
    Clear {},
    Append { recordings: Vec<Uuid> },
    Select { entry: Uuid },
    Move { entry: Uuid, before: Option<Uuid> },
    Remove { entry: Uuid },
    Replace { recordings: Vec<Uuid>, start: usize },
}
impl PlayerAction {
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Resume {} => "resume",
            Self::Pause {} => "pause",
            Self::Seek { .. } => "seek",
            Self::Volume { .. } => "volume",
            Self::Next {} => "next",
            Self::Clear {} => "clear",
            Self::Append { .. } => "append",
            Self::Select { .. } => "select",
            Self::Move { .. } => "move",
            Self::Remove { .. } => "remove",
            Self::Replace { .. } => "replace",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlayerPhase {
    #[default]
    Idle,
    Resolving,
    Opening,
    Ready,
    Releasing,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerPosition {
    pub entry: Option<Uuid>,
    pub selection: u64,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub seek_id: Option<Uuid>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Seek {
    pub id: Uuid,
    pub position_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerReceipt {
    pub id: Uuid,
    pub sequence: u64,
    pub revision: u64,
}

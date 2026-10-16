use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerSnapshot {
    pub revision: u64,
    pub selection: u64,
    pub queue_len: usize,
    pub current: Option<PlayerEntryRef>,
    pub desired_playing: bool,
    pub playing: bool,
    pub phase: PlayerPhase,
    pub error: Option<PlayerError>,
}
pub struct PlayerInspection {
    pub native: bool,
    pub epoch: Uuid,
    pub state: Option<PlayerSnapshot>,
}
pub struct PlayerAttachment {
    pub native: bool,
    pub epoch: Uuid,
    pub lease: Uuid,
    pub state: PlayerSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerEntryRef {
    pub id: Uuid,
    pub recording_id: Uuid,
}
pub struct PlayerQueueEntry {
    pub id: Uuid,
    pub recording: hito_core::Recording,
}
pub struct PlayerQueuePage {
    pub revision: u64,
    pub total: usize,
    pub offset: usize,
    pub entries: Vec<PlayerQueueEntry>,
}
#[derive(Clone, PartialEq, Eq)]
pub struct PlayerCommand {
    pub id: Uuid,
    pub lease: Uuid,
    pub sequence: u64,
    pub revision: u64,
    pub action: PlayerAction,
}
#[derive(Clone, PartialEq, Eq)]
pub enum PlayerAction {
    Resume,
    Next,
    Clear,
    Append { recordings: Vec<Uuid> },
    Select { entry: Uuid },
    Move { entry: Uuid, before: Option<Uuid> },
    Remove { entry: Uuid },
    Replace { recordings: Vec<Uuid>, start: usize },
}
impl PlayerAction {
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Resume => "resume",
            Self::Next => "next",
            Self::Clear => "clear",
            Self::Append { .. } => "append",
            Self::Select { .. } => "select",
            Self::Move { .. } => "move",
            Self::Remove { .. } => "remove",
            Self::Replace { .. } => "replace",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerPhase {
    #[default]
    Idle,
    Resolving,
    Opening,
    Ready,
    Releasing,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerPosition {
    pub entry: Option<Uuid>,
    pub selection: u64,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerReceipt {
    pub id: Uuid,
    pub sequence: u64,
    pub revision: u64,
}

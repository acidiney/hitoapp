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

#[derive(Clone, Default)]
pub struct PlayerSnapshot {
    pub revision: u64,
    pub selection: u64,
    pub queue_len: usize,
    pub current: Option<PlayerEntryRef>,
}
pub struct PlayerInspection {
    pub epoch: Uuid,
    pub state: Option<PlayerSnapshot>,
}
pub struct PlayerAttachment {
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerReceipt {
    pub id: Uuid,
    pub sequence: u64,
    pub revision: u64,
}

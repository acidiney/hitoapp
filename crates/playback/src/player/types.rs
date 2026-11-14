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
    #[error("This device can't play this audio format")]
    OutputFormat,
    #[error("Listening progress could not be saved")]
    Persistence,
    #[error("Saved playback position could not be loaded. Try again.")]
    Resume,
}
impl From<hito_core::Error> for PlayerError {
    fn from(error: hito_core::Error) -> Self {
        tracing::debug!(
            event = "player_authority_failed",
            error_kind = error.code(),
            "Playback authority check failed"
        );
        match error {
            hito_core::Error::Unauthorized | hito_core::Error::Locked => Self::Unauthorized,
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
    pub current_index: Option<usize>,
    pub current: Option<PlayerEntryRef>,
    pub can_previous: bool,
    pub repeat: PlayerRepeat,
    pub shuffle: bool,
    pub source_id: Option<String>,
    pub desired_playing: bool,
    pub playing: bool,
    pub seeking: bool,
    pub volume: u16,
    pub speed: u16,
    pub effective_speed: u16,
    pub speed_support: Option<PlayerSpeedSupport>,
    #[cfg_attr(feature = "schema", schema(required = true))]
    pub retrying: Option<PlayerRetry>,
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
            current_index: None,
            current: None,
            can_previous: false,
            repeat: PlayerRepeat::Off,
            shuffle: false,
            source_id: None,
            desired_playing: false,
            playing: false,
            seeking: false,
            volume: 1000,
            speed: 1000,
            effective_speed: 1000,
            speed_support: None,
            retrying: None,
            phase: PlayerPhase::Idle,
            error: None,
            last_failure: None,
            persistence_error: None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerRetry {
    pub attempt: u8,
    pub of: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerSpeedSupport {
    pub min: u16,
    pub max: u16,
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

#[derive(Clone, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct PlayerAudiobookRequest {
    pub id: Uuid,
    pub lease: Uuid,
    pub revision: u64,
    pub selection: u64,
    pub audiobook_id: Uuid,
}

#[derive(Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerAudiobookContinuation {
    pub token: Uuid,
    pub chapter: hito_core::AudiobookChapterAnchor,
    pub position_ms: u64,
    pub speed: u16,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerEntryRef {
    pub id: Uuid,
    pub recording_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter: Option<hito_core::AudiobookChapterAnchor>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct PlayerQueueInput {
    pub recording_id: Uuid,
    pub chapter: Option<hito_core::AudiobookChapterAnchor>,
}
impl PlayerQueueInput {
    pub(super) fn valid(&self) -> bool {
        !self.recording_id.is_nil()
            && self.chapter.is_none_or(|chapter| {
                !chapter.audiobook_id.is_nil()
                    && !chapter.entry_id.is_nil()
                    && chapter.recording_id == self.recording_id
            })
    }
}
impl From<Uuid> for PlayerQueueInput {
    fn from(recording_id: Uuid) -> Self {
        Self {
            recording_id,
            chapter: None,
        }
    }
}
#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct PlayerQueueEntry {
    pub id: Uuid,
    pub recording: hito_core::Recording,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter: Option<hito_core::AudiobookChapterAnchor>,
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
    ResumeSaved {},
    ContinueAudiobook {
        token: Uuid,
    },
    Pause {},
    Seek {
        position_ms: u64,
    },
    Volume {
        level: u16,
    },
    Speed {
        rate: u16,
    },
    Next {},
    Previous {},
    Repeat {},
    Shuffle {},
    Clear {},
    Append {
        recordings: Vec<Uuid>,
    },
    AppendEntries {
        entries: Vec<PlayerQueueInput>,
    },
    Select {
        entry: Uuid,
    },
    Move {
        entry: Uuid,
        before: Option<Uuid>,
    },
    Remove {
        entry: Uuid,
    },
    Replace {
        recordings: Vec<Uuid>,
        start: usize,
    },
    ReplaceEntries {
        entries: Vec<PlayerQueueInput>,
        start: usize,
    },
}
impl PlayerAction {
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Resume {} => "resume",
            Self::ResumeSaved {} => "resume_saved",
            Self::ContinueAudiobook { .. } => "continue_audiobook",
            Self::Pause {} => "pause",
            Self::Seek { .. } => "seek",
            Self::Volume { .. } => "volume",
            Self::Speed { .. } => "speed",
            Self::Next {} => "next",
            Self::Previous {} => "previous",
            Self::Repeat {} => "repeat",
            Self::Shuffle {} => "shuffle",
            Self::Clear {} => "clear",
            Self::Append { .. } => "append",
            Self::AppendEntries { .. } => "append_entries",
            Self::Select { .. } => "select",
            Self::Move { .. } => "move",
            Self::Remove { .. } => "remove",
            Self::Replace { .. } => "replace",
            Self::ReplaceEntries { .. } => "replace_entries",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlayerRepeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlayerPhase {
    #[default]
    Idle,
    Resolving,
    Loading,
    Opening,
    Ready,
    Releasing,
    Preparing,
    Failed,
    Suspended,
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

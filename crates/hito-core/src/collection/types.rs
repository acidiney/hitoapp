use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(ToSchema, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub id: Uuid,
    pub action: Action,
}

#[derive(ToSchema, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    ReleaseTitleSet {
        release_id: Uuid,
        title: String,
    },
    ReleaseArtistSet {
        release_id: Uuid,
        artist: String,
    },
    ReleaseTitleInherit {
        release_id: Uuid,
    },
    ReleaseArtistInherit {
        release_id: Uuid,
    },
    RecordingArtistSet {
        recording_id: Uuid,
        artist: String,
    },
    RecordingArtistInherit {
        recording_id: Uuid,
    },
    RecordingTitleSet {
        recording_id: Uuid,
        title: String,
    },
    RecordingTitleInherit {
        recording_id: Uuid,
    },
    LikeSet {
        recording_id: Uuid,
        liked: bool,
    },
    ProgressSet {
        recording_id: Uuid,
        position_ms: i64,
        speed: f64,
    },
    PlaylistCreate {
        playlist_id: Uuid,
        name: String,
    },
    PlaylistRename {
        playlist_id: Uuid,
        name: String,
    },
    PlaylistDelete {
        playlist_id: Uuid,
    },
    EntryInsert {
        playlist_id: Uuid,
        entry_id: Uuid,
        recording_id: Uuid,
        position: i64,
    },
    EntryRemove {
        playlist_id: Uuid,
        entry_id: Uuid,
    },
    EntryMove {
        playlist_id: Uuid,
        entry_id: Uuid,
        position: i64,
    },
}

impl Action {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::ReleaseTitleSet { .. } => "release_title_set",
            Self::ReleaseArtistSet { .. } => "release_artist_set",
            Self::ReleaseTitleInherit { .. } => "release_title_inherit",
            Self::ReleaseArtistInherit { .. } => "release_artist_inherit",
            Self::RecordingArtistSet { .. } => "recording_artist_set",
            Self::RecordingArtistInherit { .. } => "recording_artist_inherit",
            Self::RecordingTitleSet { .. } => "recording_title_set",
            Self::RecordingTitleInherit { .. } => "recording_title_inherit",
            Self::LikeSet { .. } => "like_set",
            Self::ProgressSet { .. } => "progress_set",
            Self::PlaylistCreate { .. } => "playlist_create",
            Self::PlaylistRename { .. } => "playlist_rename",
            Self::PlaylistDelete { .. } => "playlist_delete",
            Self::EntryInsert { .. } => "entry_insert",
            Self::EntryRemove { .. } => "entry_remove",
            Self::EntryMove { .. } => "entry_move",
        }
    }
}

#[derive(ToSchema, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncBatch {
    pub device_id: String,
    pub operations: Vec<Operation>,
}

#[derive(ToSchema, Serialize, sqlx::FromRow)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(ToSchema, Serialize)]
pub struct SyncReceipt {
    pub acknowledged: Vec<Uuid>,
}

#[derive(ToSchema, Serialize)]
pub struct SyncEvent {
    pub sequence: i64,
    pub device_id: String,
    pub operation: Operation,
}

#[derive(ToSchema, Serialize)]
pub struct SyncPage {
    pub operations: Vec<SyncEvent>,
    pub cursor: i64,
    pub has_more: bool,
}

#[derive(ToSchema, Serialize)]
pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub entries: Vec<PlaylistEntry>,
}

#[derive(ToSchema, Serialize, sqlx::FromRow)]
pub struct PlaylistEntry {
    pub entry_id: String,
    pub liked: bool,
    pub recording_id: String,
    pub position: i64,
    pub title: String,
    pub inherited_title: String,
    pub inherited_artist: String,
    pub artist: String,
    pub duration_ms: i64,
    pub available: bool,
}

#[derive(Serialize, ToSchema)]
pub struct Activity {
    pub history: Vec<HistoryEntry>,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct HistoryEntry {
    pub id: String,
    pub title: String,
    pub inherited_title: String,
    pub inherited_artist: String,
    pub artist: String,
    pub duration_ms: i64,
    pub position_ms: i64,
    pub speed: f64,
    pub updated_at: i64,
    pub available: bool,
    pub liked: bool,
}

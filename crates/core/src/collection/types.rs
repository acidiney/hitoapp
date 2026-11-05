use super::{
    AudiobookChapterAnchor, CollectionPublication, ImportedPlaylistEntryOrigin,
    ImportedPlaylistOrigin,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedPosition {
    pub position_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, ToSchema)]
pub struct RecordingProgress {
    pub recording_id: Uuid,
    pub position_ms: u64,
    pub speed: f64,
}

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
    AudiobookCreate {
        playlist_id: Uuid,
    },
    AudiobookChapterSet {
        chapter: AudiobookChapterAnchor,
    },
    AudiobookBookmarkCreate {
        audiobook_id: Uuid,
        bookmark_id: Uuid,
        entry_id: Uuid,
        recording_id: Uuid,
        position_ms: i64,
        label: String,
    },
    AudiobookBookmarkRemove {
        audiobook_id: Uuid,
        bookmark_id: Uuid,
    },
    PlaylistFollowResume {
        playlist_id: Uuid,
        expected_generation: i64,
        installation_revision: i64,
        service_revision: i64,
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
            Self::AudiobookCreate { .. } => "audiobook_create",
            Self::AudiobookChapterSet { .. } => "audiobook_chapter_set",
            Self::AudiobookBookmarkCreate { .. } => "audiobook_bookmark_create",
            Self::AudiobookBookmarkRemove { .. } => "audiobook_bookmark_remove",
            Self::PlaylistFollowResume { .. } => "playlist_follow_resume",
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
#[serde(untagged)]
pub enum SyncEvent {
    Operation {
        sequence: i64,
        device_id: String,
        operation: Operation,
    },
    Publication {
        sequence: i64,
        publication: CollectionPublication,
    },
}

impl SyncEvent {
    pub fn sequence(&self) -> i64 {
        match self {
            Self::Operation { sequence, .. } | Self::Publication { sequence, .. } => *sequence,
        }
    }
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
    pub origin: Option<ImportedPlaylistOrigin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_content_revision: Option<i64>,
}

#[derive(ToSchema, Serialize)]
pub struct PlaylistEntry {
    pub entry_id: String,
    pub liked: bool,
    pub recording_id: Option<String>,
    pub position: i64,
    pub title: String,
    pub inherited_title: String,
    pub inherited_artist: String,
    pub artist: String,
    pub duration_ms: i64,
    pub available: bool,
    pub plays_from_services: bool,
    pub origin: Option<ImportedPlaylistEntryOrigin>,
}

#[derive(Serialize, ToSchema)]
pub struct Activity {
    pub history: Vec<HistoryEntry>,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct HistoryEntry {
    pub id: String,
    pub release_id: Option<String>,
    pub album: Option<String>,
    pub title: String,
    pub inherited_title: String,
    pub inherited_artist: String,
    pub artist: String,
    pub duration_ms: i64,
    pub position_ms: i64,
    pub speed: f64,
    pub updated_at: i64,
    pub available: bool,
    pub plays_from_services: bool,
    pub liked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct HistoryPreference {
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateHistoryPreference {
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct HistoryCleared {
    pub removed: u64,
}

#[derive(Serialize, ToSchema)]
pub struct Home {
    pub has_music: bool,
    pub liked_count: i64,
    pub recent_albums: Vec<HomeAlbum>,
    pub recently_added: Vec<AddedAlbum>,
    pub unheard: Vec<UnheardOnAlbum>,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct HomeAlbum {
    pub id: String,
    pub title: String,
    #[schema(required = true)]
    pub artist: Option<String>,
    #[sqlx(skip)]
    #[schema(required = true)]
    pub inferred_artist: Option<crate::AlbumArtistInference>,
    pub last_played_at: i64,
    pub played: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct AddedAlbum {
    pub id: String,
    pub title: String,
    #[schema(required = true)]
    pub artist: Option<String>,
    #[sqlx(skip)]
    #[schema(required = true)]
    pub inferred_artist: Option<crate::AlbumArtistInference>,
}

#[derive(Serialize, ToSchema)]
pub struct UnheardOnAlbum {
    pub release_id: String,
    pub title: String,
    #[schema(required = true)]
    pub artist: Option<String>,
    #[schema(required = true)]
    pub inferred_artist: Option<crate::AlbumArtistInference>,
    pub played: i64,
    pub remaining: i64,
    pub tracks: Vec<crate::Recording>,
}

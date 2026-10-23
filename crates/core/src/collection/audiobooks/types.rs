use super::super::{Playlist, PlaylistEntry, RecordingProgress};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct AudiobookDetail {
    pub playlist: Playlist,
    pub chapters: Vec<PlaylistEntry>,
    pub bookmarks: Vec<AudiobookBookmark>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation: Option<AudiobookContinuation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AudiobookChapterAnchor {
    pub audiobook_id: Uuid,
    pub entry_id: Uuid,
    pub recording_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct AudiobookContinuation {
    pub chapter: AudiobookChapterAnchor,
    pub progress: Option<RecordingProgress>,
}

#[derive(Serialize, ToSchema)]
pub struct AudiobookPlayback {
    pub audiobook_id: Uuid,
    pub chapter: AudiobookChapterAnchor,
    pub progress: RecordingProgress,
    pub chapters: Vec<PlaylistEntry>,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct AudiobookBookmark {
    pub id: String,
    pub audiobook_id: String,
    pub entry_id: String,
    pub recording_id: String,
    #[schema(minimum = 0)]
    pub position_ms: i64,
    pub label: String,
}

#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AudiobookLibraryQuery {
    #[param(minimum = 1, maximum = 100)]
    pub limit: Option<u16>,
    #[param(max_length = 4096)]
    pub cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct AudiobookPagination {
    #[schema(minimum = 1, maximum = 100)]
    pub limit: u16,
    pub next_cursor: Option<String>,
    pub previous_cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct AudiobookLibraryPage {
    pub items: Vec<Playlist>,
    pub pagination: AudiobookPagination,
}

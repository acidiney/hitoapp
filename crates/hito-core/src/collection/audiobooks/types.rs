use super::super::{Playlist, PlaylistEntry};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct AudiobookDetail {
    pub playlist: Playlist,
    pub chapters: Vec<PlaylistEntry>,
    pub bookmarks: Vec<AudiobookBookmark>,
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

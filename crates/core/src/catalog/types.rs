use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct Recording {
    pub id: String,
    pub library_id: String,
    pub release_id: Option<String>,
    pub title: String,
    pub inherited_title: String,
    pub inherited_artist: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: i64,
    pub available: bool,
    pub liked: bool,
}

#[derive(Deserialize, Default, ToSchema)]
pub struct Search {
    pub artist: Option<String>,
    pub q: Option<String>,
    pub liked: Option<bool>,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
    pub library: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct Source {
    pub id: String,
    pub recording_id: String,
    pub provider: String,
    pub label: String,
    pub device_id: Option<String>,
    pub ready: bool,
    pub can_prepare: bool,
}

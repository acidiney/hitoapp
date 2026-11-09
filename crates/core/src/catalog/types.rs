use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct Recording {
    pub id: String,
    pub library_id: String,
    pub release_id: Option<String>,
    pub title: String,
    pub inherited_title: String,
    pub inherited_artist: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub album: Option<String>,
    pub duration_ms: i64,
    pub available: bool,
    pub plays_from_services: bool,
    pub liked: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSort {
    #[default]
    Title,
    Recent,
    Artist,
    Year,
}

#[derive(Clone, Deserialize, Default, ToSchema)]
pub struct Search {
    pub sort: Option<CatalogSort>,
    pub artist: Option<String>,
    #[serde(skip)]
    pub featured_artist: Option<String>,
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
    pub provider_label: Option<String>,
    pub device_id: Option<String>,
    pub ready: bool,
    pub can_prepare: bool,
    pub match_kind: SourceMatchKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, sqlx::Type, ToSchema)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum SourceMatchKind {
    Verified,
    BestGuess,
}

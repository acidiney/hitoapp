use serde::Serialize;
use std::path::PathBuf;
use utoipa::ToSchema;

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct Job {
    pub id: String,
    pub library_id: String,
    pub folder_id: Option<String>,
    pub kind: String,
    pub state: String,
    pub progress: i64,
    pub error: Option<String>,
}

pub(super) struct IndexedFile {
    pub path: PathBuf,
    pub checksum: String,
    pub audio: Option<AudioEvidence>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub duration_ms: i64,
    pub disc_number: Option<i64>,
    pub track_number: Option<i64>,
    pub tag_details: Option<String>,
    pub size: i64,
}

pub(super) struct AudioEvidence {
    pub algorithm: &'static str,
    pub digest: String,
}

pub(super) const AUDIO_ALGORITHM: &str = "ffmpeg-pcm-f64-v1";
pub(super) const METADATA_VERSION: i64 = 2;

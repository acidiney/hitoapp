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
    #[sqlx(skip)]
    pub failure: Option<JobFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobFailure {
    MediaToolsMissing,
}

pub(super) const MEDIA_TOOLS_MISSING_MESSAGE: &str =
    "FFmpeg is not installed. Install it (for example `brew install ffmpeg`) and scan again.";

pub(super) const TEMP_STORAGE_FULL_MESSAGE: &str = "There is not enough temporary disk space to scan. Free space, or lower the scan temporary budget, and scan again.";

impl JobFailure {
    pub(super) fn recorded(message: Option<&str>) -> Option<Self> {
        (message == Some(MEDIA_TOOLS_MISSING_MESSAGE)).then_some(Self::MediaToolsMissing)
    }
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
    pub credits: Vec<hito_plugin_sdk::CreditedArtist>,
    pub size: i64,
}

pub(super) struct AudioEvidence {
    pub algorithm: &'static str,
    pub digest: String,
}

pub(super) const AUDIO_ALGORITHM: &str = "ffmpeg-pcm-f64-v1";
pub(super) const METADATA_VERSION: i64 = 8;

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Debug, Serialize, ToSchema)]
pub struct PlaylistDownloadReceipt {
    pub request_id: Uuid,
    pub retention_id: String,
    pub playlist_id: String,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct PlaylistDownloadQuery {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

impl PlaylistDownloadQuery {
    pub(super) fn selection(
        &self,
        retention: &str,
        revision: i64,
    ) -> Result<(Option<String>, i64)> {
        let limit = self.limit.unwrap_or(32);
        if !(1..=100).contains(&limit) {
            return Err(Error::Invalid(
                "Use a playlist download page size between 1 and 100".into(),
            ));
        }
        let Some(cursor) = &self.cursor else {
            return Ok((None, i64::from(limit)));
        };
        if cursor.len() > 128 {
            return Err(invalid_cursor());
        }
        let mut parts = cursor.split(':');
        let root = Uuid::parse_str(parts.next().ok_or_else(invalid_cursor)?)
            .map_err(|_| invalid_cursor())?;
        let cursor_revision = parts
            .next()
            .ok_or_else(invalid_cursor)?
            .parse::<i64>()
            .map_err(|_| invalid_cursor())?;
        let after = Uuid::parse_str(parts.next().ok_or_else(invalid_cursor)?)
            .map_err(|_| invalid_cursor())?;
        if parts.next().is_some() || cursor_revision <= 0 {
            return Err(invalid_cursor());
        }
        if root.to_string() != retention || cursor_revision != revision {
            return Err(Error::Conflict(
                "Playlist download membership changed. Start a new page sequence.".into(),
            ));
        }
        Ok((Some(after.to_string()), i64::from(limit)))
    }
}

fn invalid_cursor() -> Error {
    Error::Invalid("Use the playlist download cursor returned by Hito".into())
}

#[derive(Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlaylistDownloadStatus {
    pub retention_id: String,
    pub playlist_id: String,
    pub host_device_id: String,
    pub desired_revision: i64,
    pub reconciled_revision: i64,
    pub entries: PlaylistDownloadEntries,
}

#[derive(Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlaylistDownloadEntries {
    pub items: Vec<PlaylistDownloadEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlaylistDownloadEntry {
    pub entry_id: String,
    pub recording_id: Option<String>,
    pub status: PlaylistDownloadEntryState,
    pub source_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistDownloadEntryState {
    Pending,
    WaitingForSource,
    Queued,
    Running,
    Verifying,
    Ready,
    Failed,
    Unavailable,
}

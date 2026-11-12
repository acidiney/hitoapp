use crate::{ArtworkChangeEntity, ChangeNotice};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use sqlx::{FromRow, Row, sqlite::SqliteRow};

pub(super) enum Target {
    Recording {
        id: String,
        provider_id: String,
        observation_id: String,
    },
    Release {
        id: String,
        provider_id: String,
        observation_id: String,
    },
    Playlist {
        id: String,
        provider_id: String,
    },
}

impl Target {
    pub(super) fn change_notice(&self) -> ChangeNotice {
        let (entity, id) = match self {
            Self::Recording { id, .. } => (ArtworkChangeEntity::Recording, id),
            Self::Release { id, .. } => (ArtworkChangeEntity::Release, id),
            Self::Playlist { id, .. } => (ArtworkChangeEntity::Playlist, id),
        };
        ChangeNotice::Artwork {
            entity,
            id: id.clone(),
        }
    }

    pub(super) fn request(&self) -> ArtworkRequest {
        match self {
            Self::Recording { provider_id, .. } => ArtworkRequest {
                entity: ArtworkEntity::Recording,
                provider_id: provider_id.clone(),
            },
            Self::Release { provider_id, .. } => ArtworkRequest {
                entity: ArtworkEntity::Release,
                provider_id: provider_id.clone(),
            },
            Self::Playlist { provider_id, .. } => ArtworkRequest {
                entity: ArtworkEntity::Playlist,
                provider_id: provider_id.clone(),
            },
        }
    }
}

impl<'row> FromRow<'row, SqliteRow> for Target {
    fn from_row(row: &'row SqliteRow) -> std::result::Result<Self, sqlx::Error> {
        match row.try_get::<String, _>("entity_kind")?.as_str() {
            "recording" => Ok(Self::Recording {
                id: row.try_get("recording_id")?,
                provider_id: row.try_get("provider_recording_id")?,
                observation_id: row.try_get("observation_id")?,
            }),
            "release" => Ok(Self::Release {
                id: row.try_get("release_id")?,
                provider_id: row.try_get("provider_release_id")?,
                observation_id: row.try_get("observation_id")?,
            }),
            "playlist" => Ok(Self::Playlist {
                id: row.try_get("playlist_id")?,
                provider_id: row.try_get("provider_playlist_id")?,
            }),
            _ => Err(sqlx::Error::ColumnDecode {
                index: "entity_kind".into(),
                source: "Stored artwork entity is invalid".into(),
            }),
        }
    }
}

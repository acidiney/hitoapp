use super::{
    Catalog, Recording,
    genres::{GenrePosition, GenreWindow, Key, genre_identity, genre_recordings},
    recordings_on,
};
use crate::{Actor, Error, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite};
use utoipa::ToSchema;
use uuid::Uuid;

const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 200;
const CURSOR_VERSION: u8 = 1;
const MAX_CURSOR_LENGTH: usize = 1_024;
pub(super) const ANCHOR_TITLE_SQL: &str = "SELECT r.title FROM recordings r
                JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ?
                WHERE r.id = ?";

#[derive(Serialize, ToSchema)]
pub struct GenreTracksPage {
    pub items: Vec<Recording>,
    #[schema(required = true, nullable = true)]
    pub next_after: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GenreTracksCursor {
    version: u8,
    account: String,
    key: String,
    last_recording: String,
}

fn invalid_cursor() -> Error {
    Error::Invalid("Restart genre track pagination".into())
}

fn decode_cursor(cursor: &str, account: &str, key: &str) -> Result<Uuid> {
    if cursor.is_empty() || cursor.len() > MAX_CURSOR_LENGTH {
        return Err(invalid_cursor());
    }
    let payload = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| invalid_cursor())?;
    if URL_SAFE_NO_PAD.encode(&payload) != cursor {
        return Err(invalid_cursor());
    }
    let cursor: GenreTracksCursor =
        serde_json::from_slice(&payload).map_err(|_| invalid_cursor())?;
    if cursor.version != CURSOR_VERSION || cursor.account != account || cursor.key != key {
        return Err(invalid_cursor());
    }
    let last_recording = Uuid::parse_str(&cursor.last_recording).map_err(|_| invalid_cursor())?;
    if last_recording.is_nil() || last_recording.to_string() != cursor.last_recording {
        return Err(invalid_cursor());
    }
    Ok(last_recording)
}

fn encode_cursor(account: &str, key: &str, last_recording: Uuid) -> Result<String> {
    let payload = serde_json::to_vec(&GenreTracksCursor {
        version: CURSOR_VERSION,
        account: account.to_owned(),
        key: key.to_owned(),
        last_recording: last_recording.to_string(),
    })
    .map_err(|_| Error::Internal("Genre continuation could not be encoded".into()))?;
    let cursor = URL_SAFE_NO_PAD.encode(payload);
    if cursor.len() > MAX_CURSOR_LENGTH {
        return Err(Error::Internal(
            "Genre continuation exceeds its size limit".into(),
        ));
    }
    Ok(cursor)
}

pub(super) fn accessible_query(
    host: &crate::HostDeviceId,
    actor: &Actor,
    key: &str,
) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::<Sqlite>::new("SELECT id FROM (");
    genre_recordings(
        &mut query,
        host,
        actor,
        &Key::Bound(key),
        "r.id AS id",
        GenreWindow::Preview { limit: 1 },
    );
    query.push(") LIMIT 1");
    query
}

pub(super) fn candidates_query(
    host: &crate::HostDeviceId,
    actor: &Actor,
    key: &str,
    limit: i64,
    after: Option<GenrePosition<'_>>,
) -> QueryBuilder<'static, Sqlite> {
    let window_limit = limit + 1;
    let mut query = QueryBuilder::<Sqlite>::new("SELECT id, title FROM (");
    genre_recordings(
        &mut query,
        host,
        actor,
        &Key::Bound(key),
        "r.id AS id, r.title AS title",
        GenreWindow::Page {
            limit: window_limit,
            after,
        },
    );
    query
        .push(") GROUP BY id ORDER BY title COLLATE NOCASE, id LIMIT ")
        .push_bind(window_limit);
    query
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn genre_tracks(
        &self,
        actor: &Actor,
        name: &str,
        limit: Option<i64>,
        after: Option<&str>,
    ) -> Result<GenreTracksPage> {
        let (key, _) = genre_identity(name).ok_or(Error::NotFound)?;
        let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;

        let anchor = after
            .map(|cursor| decode_cursor(cursor, actor.id(), &key))
            .transpose()?;

        let accessible: Option<String> = accessible_query(&self.host, actor, &key)
            .build_query_scalar()
            .fetch_optional(&mut *tx)
            .await?;
        if accessible.is_none() {
            return Err(Error::NotFound);
        }

        let (anchor_id, anchor_title) = if let Some(anchor) = anchor {
            let title: Option<String> = sqlx::query_scalar(ANCHOR_TITLE_SQL)
                .bind(actor.id().to_owned())
                .bind(anchor.to_string())
                .fetch_optional(&mut *tx)
                .await?;
            (
                Some(anchor.to_string()),
                Some(title.ok_or_else(invalid_cursor)?),
            )
        } else {
            (None, None)
        };
        let after_position = anchor_title
            .as_deref()
            .zip(anchor_id.as_deref())
            .map(|(title, id)| GenrePosition { title, id });

        let candidates: Vec<(String, String)> = candidates_query(
            &self.host,
            actor,
            &key,
            limit,
            after_position,
        )
        .build_query_as()
        .fetch_all(&mut *tx)
        .await?;
        let has_more = candidates.len() > limit as usize;
        let candidates: Vec<(String, String)> =
            candidates.into_iter().take(limit as usize).collect();
        let last_recording = if has_more {
            let id = candidates
                .last()
                .map(|(id, _)| id.as_str())
                .ok_or_else(|| Error::Internal("Genre continuation has no anchor".into()))?;
            Some(
                Uuid::parse_str(id)
                    .map_err(|_| Error::Internal("Stored recording identity is invalid".into()))?,
            )
        } else {
            None
        };
        let recording_ids = candidates
            .iter()
            .map(|(id, _)| {
                Uuid::parse_str(id)
                    .map_err(|_| Error::Internal("Stored recording identity is invalid".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let items = recordings_on(&mut tx, &self.host, actor, &recording_ids).await?;
        let next_after = last_recording
            .map(|id| encode_cursor(actor.id(), &key, id))
            .transpose()?;

        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::debug!(items = items.len(), has_more, "Genre tracks read");
        Ok(GenreTracksPage { items, next_after })
    }
}

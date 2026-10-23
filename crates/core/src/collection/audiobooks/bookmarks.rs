use super::{MAX_BOOKMARKS, editable, owned_on};
use crate::{Actor, Error, Result, identity::recording_access};
use std::time::Instant;
use uuid::Uuid;

pub(in crate::collection) struct Create<'a> {
    pub audiobook_id: Uuid,
    pub bookmark_id: Uuid,
    pub entry_id: Uuid,
    pub recording_id: Uuid,
    pub position_ms: i64,
    pub label: &'a str,
}

#[derive(sqlx::FromRow)]
struct StoredBookmark {
    audiobook_id: String,
    entry_id: Option<String>,
    recording_id: Option<String>,
    position_ms: Option<i64>,
    label: Option<String>,
    deleted: bool,
}

#[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_bookmark_create", request_id = %operation_id))]
pub(in crate::collection) async fn create(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    request: Create<'_>,
    operation_id: Uuid,
) -> Result<()> {
    let started = Instant::now();
    let result: Result<()> = async {
        if request.label.len() > 256 {
            return Err(Error::Invalid("Use a bookmark label of 1 to 256 bytes".into()));
        }
        let label = request.label.trim();
        if request.audiobook_id.is_nil() || request.bookmark_id.is_nil()
            || request.entry_id.is_nil() || request.recording_id.is_nil()
            || request.position_ms < 0 || label.is_empty() || label.len() > 256 {
            return Err(Error::Invalid("Use nonempty bookmark identities, a nonnegative position and a label of 1 to 256 bytes".into()));
        }
        if !editable(tx, actor, &request.audiobook_id).await? {
            return Ok(());
        }
        let audiobook = request.audiobook_id.to_string();
        let entry = request.entry_id.to_string();
        let recording = request.recording_id.to_string();
        let bookmark = request.bookmark_id.to_string();
        owned_on(tx, actor, &audiobook).await?;
        let previous = sqlx::query_as::<_, StoredBookmark>(
            "SELECT audiobook_id,entry_id,recording_id,position_ms,label,deleted FROM audiobook_bookmarks WHERE user_id=? AND id=?",
        ).bind(actor.id()).bind(&bookmark).fetch_optional(&mut **tx).await?;
        if let Some(previous) = &previous {
            if previous.audiobook_id != audiobook {
                return Err(Error::Conflict("This bookmark identity was already used for another book".into()));
            }
            if previous.deleted {
                return Ok(());
            }
        }
        let chapter_matches: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playlist_entries WHERE playlist_id=? AND id=? AND recording_id=? AND deleted=0)")
            .bind(&audiobook).bind(&entry).bind(&recording).fetch_one(&mut **tx).await?;
        if !chapter_matches {
            return Err(Error::NotFound);
        }
        recording_access(tx, actor, &recording).await?;
        if let Some(previous) = previous {
            if previous.entry_id.as_deref() != Some(entry.as_str())
                || previous.recording_id.as_deref() != Some(recording.as_str())
                || previous.position_ms != Some(request.position_ms)
                || previous.label.as_deref() != Some(label) {
                return Err(Error::Conflict("This bookmark identity was already used for another position".into()));
            }
            return Ok(());
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audiobook_bookmarks WHERE user_id=? AND audiobook_id=? AND deleted=0")
            .bind(actor.id()).bind(&audiobook).fetch_one(&mut **tx).await?;
        if count >= MAX_BOOKMARKS {
            return Err(Error::Invalid("An audiobook can contain at most 256 bookmarks".into()));
        }
        sqlx::query("INSERT INTO audiobook_bookmarks(user_id,id,audiobook_id,entry_id,recording_id,position_ms,label) VALUES(?,?,?,?,?,?,?)")
            .bind(actor.id()).bind(bookmark).bind(audiobook).bind(entry).bind(recording)
            .bind(request.position_ms).bind(label).execute(&mut **tx).await?;
        Ok(())
    }.await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Audiobook bookmark creation completed"
    );
    result
}

#[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_bookmark_remove", request_id = %operation_id))]
pub(in crate::collection) async fn remove(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    audiobook_id: &Uuid,
    bookmark_id: &Uuid,
    operation_id: Uuid,
) -> Result<()> {
    let started = Instant::now();
    let result: Result<()> = async {
        if audiobook_id.is_nil() || bookmark_id.is_nil() {
            return Err(Error::Invalid(
                "Use nonempty audiobook and bookmark identities".into(),
            ));
        }
        if !editable(tx, actor, audiobook_id).await? {
            return Ok(());
        }
        let audiobook = audiobook_id.to_string();
        let bookmark = bookmark_id.to_string();
        owned_on(tx, actor, &audiobook).await?;
        let previous: Option<String> = sqlx::query_scalar(
            "SELECT audiobook_id FROM audiobook_bookmarks WHERE user_id=? AND id=?",
        )
        .bind(actor.id())
        .bind(&bookmark)
        .fetch_optional(&mut **tx)
        .await?;
        if previous.is_some_and(|previous| previous != audiobook) {
            return Err(Error::NotFound);
        }
        sqlx::query(
            "INSERT INTO audiobook_bookmarks(user_id,id,audiobook_id,deleted) VALUES(?,?,?,1)
            ON CONFLICT(user_id,id) DO UPDATE SET deleted=1 WHERE audiobook_bookmarks.deleted=0",
        )
        .bind(actor.id())
        .bind(bookmark)
        .bind(audiobook)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
    .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Audiobook bookmark removal completed"
    );
    result
}

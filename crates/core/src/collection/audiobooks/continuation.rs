use super::{AudiobookChapterAnchor, AudiobookContinuation, editable, owned_on};
use crate::{Actor, Error, RecordingProgress, Result, identity::recording_access};
use sqlx::SqliteConnection;
use std::time::Instant;
use uuid::Uuid;

#[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_chapter_set", request_id = %operation_id))]
pub(in crate::collection) async fn set(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    chapter: &AudiobookChapterAnchor,
    operation_id: Uuid,
) -> Result<()> {
    let started = Instant::now();
    let result: Result<&'static str> = async {
        if chapter.audiobook_id.is_nil()
            || chapter.entry_id.is_nil()
            || chapter.recording_id.is_nil()
        {
            return Err(Error::Invalid(
                "Use nonempty audiobook, chapter and recording identities".into(),
            ));
        }
        if !editable(tx, actor, &chapter.audiobook_id).await? {
            return Ok("deleted_book");
        }
        let audiobook = chapter.audiobook_id.to_string();
        let entry = chapter.entry_id.to_string();
        let recording = chapter.recording_id.to_string();
        owned_on(tx, actor, &audiobook).await?;
        let stored: Option<(Option<String>, bool)> = sqlx::query_as(
            "SELECT recording_id,deleted FROM playlist_entries WHERE playlist_id=? AND id=?",
        )
        .bind(&audiobook)
        .bind(&entry)
        .fetch_optional(&mut **tx)
        .await?;
        match stored {
            Some((_, true)) => return Ok("deleted_chapter"),
            Some((Some(stored), false)) if stored == recording => (),
            _ => return Err(Error::NotFound),
        }
        recording_access(tx, actor, &recording).await?;
        sqlx::query(
            "INSERT INTO audiobook_continuations(user_id,audiobook_id,entry_id,recording_id)
             VALUES(?,?,?,?) ON CONFLICT(user_id,audiobook_id) DO UPDATE SET
                 entry_id=excluded.entry_id,recording_id=excluded.recording_id",
        )
        .bind(actor.id())
        .bind(audiobook)
        .bind(entry)
        .bind(recording)
        .execute(&mut **tx)
        .await?;
        Ok("selected")
    }
    .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .copied()
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Audiobook chapter selection completed"
    );
    result.map(|_| ())
}

pub(super) async fn read(
    connection: &mut SqliteConnection,
    actor: &Actor,
    audiobook: &str,
) -> Result<Option<AudiobookContinuation>> {
    let stored: Option<(String, String, Option<i64>, Option<f64>)> = sqlx::query_as(
        "SELECT continuation.entry_id,continuation.recording_id,progress.position_ms,progress.speed
         FROM audiobook_continuations continuation
         JOIN recordings recording ON recording.id=continuation.recording_id
         LEFT JOIN library_members access ON access.library_id=recording.library_id
             AND access.user_id=continuation.user_id
         LEFT JOIN listening_progress progress ON progress.recording_id=recording.id
             AND progress.user_id=access.user_id
         WHERE continuation.user_id=? AND continuation.audiobook_id=?",
    )
    .bind(actor.id())
    .bind(audiobook)
    .fetch_optional(connection)
    .await?;
    let Some((entry, recording, position, speed)) = stored else {
        return Ok(None);
    };
    let chapter = AudiobookChapterAnchor {
        audiobook_id: stored_identity(audiobook)?,
        entry_id: stored_identity(&entry)?,
        recording_id: stored_identity(&recording)?,
    };
    let progress = position
        .zip(speed)
        .map(|(position_ms, speed)| RecordingProgress {
            recording_id: chapter.recording_id,
            position_ms: position_ms as u64,
            speed,
        });
    Ok(Some(AudiobookContinuation { chapter, progress }))
}

fn stored_identity(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|_| Error::Internal("Stored audiobook chapter identity is invalid".into()))
}

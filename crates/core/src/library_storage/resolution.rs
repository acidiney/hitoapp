use super::types::IndexedFile;
use crate::{Error, Result};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(super) struct LocalIdentity {
    pub source: String,
    pub recording: String,
    pub device: Option<String>,
    pub checksum: Option<String>,
}

pub(super) async fn resolve(
    tx: &mut Transaction<'_, Sqlite>,
    library: &str,
    release: &str,
    file: &IndexedFile,
    previous: Option<&LocalIdentity>,
) -> Result<String> {
    if let Some(previous) = previous
        && (previous.checksum.as_deref() == Some(&file.checksum)
            || matches(tx, &previous.recording, file).await?)
    {
        return Ok(previous.recording.clone());
    }
    let initial: Option<String> = sqlx::query_scalar(
        "SELECT id FROM recordings WHERE library_id = ? AND initial_file_checksum = ?",
    )
    .bind(library)
    .bind(&file.checksum)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(recording) = initial {
        return Ok(recording);
    }
    let exact: Option<String> = sqlx::query_scalar(
        "SELECT s.recording_id FROM local_files f JOIN sources s ON s.id = f.source_id
         WHERE f.library_id = ? AND f.file_checksum = ? ORDER BY f.source_id LIMIT 1",
    )
    .bind(library)
    .bind(&file.checksum)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(recording) = exact {
        return Ok(recording);
    }
    if let Some(audio) = &file.audio {
        let candidates: Vec<String> = sqlx::query_scalar(
            "SELECT e.recording_id FROM recording_evidence e
             JOIN recordings r ON r.id = e.recording_id
             WHERE r.library_id = ? AND e.algorithm = ? AND e.digest = ? LIMIT 2",
        )
        .bind(library)
        .bind(audio.algorithm)
        .bind(&audio.digest)
        .fetch_all(&mut **tx)
        .await?;
        if candidates.len() == 1 {
            return Ok(candidates[0].clone());
        }
        if candidates.len() > 1 {
            tracing::debug!(
                event = "recording_match_ambiguous",
                "Historical recording identities kept separate"
            );
        }
    }
    let recording = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO recordings(id, library_id, release_id, title, artist, duration_ms, initial_file_checksum)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    ).bind(&recording).bind(library).bind(release).bind(&file.title).bind(&file.artist)
        .bind(file.duration_ms).bind(&file.checksum).execute(&mut **tx).await?;
    tracing::debug!(event = "recording_created", recording_id = %recording, "Recording created from verified file");
    Ok(recording)
}

async fn matches(
    tx: &mut Transaction<'_, Sqlite>,
    recording: &str,
    file: &IndexedFile,
) -> Result<bool> {
    let Some(audio) = &file.audio else {
        return Ok(false);
    };
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM recording_evidence WHERE recording_id = ? AND algorithm = ? AND digest = ?)",
    ).bind(recording).bind(audio.algorithm).bind(&audio.digest).fetch_one(&mut **tx).await?)
}

pub(super) fn check_host(previous: Option<&LocalIdentity>, host: &str) -> Result<()> {
    if previous.is_some_and(|file| file.device.as_deref() != Some(host)) {
        return Err(Error::Conflict(
            "This local file belongs to another host device".into(),
        ));
    }
    Ok(())
}

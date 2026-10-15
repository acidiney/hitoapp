use super::types::IndexedFile;
use crate::Result;
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, Transaction};
use std::path::Path;
use uuid::Uuid;

pub(super) async fn release(
    tx: &mut Transaction<'_, Sqlite>,
    library: &str,
    root: &Path,
    file: &IndexedFile,
) -> Result<String> {
    let folder = file.path.parent().unwrap_or(root).to_string_lossy();
    let mut key = Sha256::new();
    for value in [folder.as_ref(), &file.album, &file.album_artist] {
        key.update((value.len() as u64).to_le_bytes());
        key.update(value.as_bytes());
    }
    let key = format!("{:x}", key.finalize());
    sqlx::query("INSERT OR IGNORE INTO releases VALUES (?, ?, ?, ?, ?)")
        .bind(Uuid::new_v4().to_string())
        .bind(library)
        .bind(&file.album)
        .bind(&file.album_artist)
        .bind(&key)
        .execute(&mut **tx)
        .await?;
    Ok(
        sqlx::query_scalar("SELECT id FROM releases WHERE library_id = ? AND identity_key = ?")
            .bind(library)
            .bind(key)
            .fetch_one(&mut **tx)
            .await?,
    )
}

pub(super) async fn remember(
    tx: &mut Transaction<'_, Sqlite>,
    recording: &str,
    source: &str,
    release: &str,
    file: &IndexedFile,
) -> Result<()> {
    if let Some(audio) = &file.audio {
        sqlx::query("INSERT OR IGNORE INTO recording_evidence(recording_id, algorithm, digest) VALUES (?, ?, ?)")
            .bind(recording).bind(audio.algorithm).bind(&audio.digest).execute(&mut **tx).await?;
    }
    sqlx::query(
        "INSERT INTO source_metadata(source_id, release_id, title, artist, duration_ms) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(source_id) DO UPDATE SET release_id = excluded.release_id, title = excluded.title,
            artist = excluded.artist, duration_ms = excluded.duration_ms",
    ).bind(source).bind(release).bind(&file.title).bind(&file.artist).bind(file.duration_ms).execute(&mut **tx).await?;
    refresh_metadata(tx, recording).await
}

#[derive(sqlx::FromRow)]
struct Metadata {
    source: String,
    release_id: Option<String>,
    title: String,
    artist: String,
    duration_ms: i64,
}

pub(super) async fn refresh_metadata(
    tx: &mut Transaction<'_, Sqlite>,
    recording: &str,
) -> Result<()> {
    let current: Option<Metadata> = sqlx::query_as(
        "SELECT s.id AS source, m.release_id, m.title, m.artist, m.duration_ms
         FROM recordings r JOIN sources s ON s.id = r.metadata_source_id
         JOIN source_metadata m ON m.source_id = s.id JOIN local_files f ON f.source_id = s.id
         WHERE r.id = ? AND s.recording_id = r.id AND f.library_id = r.library_id AND s.ready = 1",
    )
    .bind(recording)
    .fetch_optional(&mut **tx)
    .await?;
    let metadata = if current.is_some() {
        current
    } else {
        sqlx::query_as(
            "SELECT s.id AS source, m.release_id, m.title, m.artist, m.duration_ms
             FROM sources s JOIN source_metadata m ON m.source_id = s.id
             JOIN local_files f ON f.source_id = s.id JOIN recordings r ON r.id = s.recording_id
             WHERE r.id = ? AND f.library_id = r.library_id AND s.ready = 1 ORDER BY s.id LIMIT 1",
        )
        .bind(recording)
        .fetch_optional(&mut **tx)
        .await?
    };
    let Some(metadata) = metadata else {
        sqlx::query("UPDATE recordings SET metadata_source_id = NULL WHERE id = ? AND metadata_source_id IS NOT NULL")
            .bind(recording).execute(&mut **tx).await?;
        return Ok(());
    };
    sqlx::query(
        "UPDATE recordings SET metadata_source_id = ?, release_id = ?, title = ?, artist = ?, duration_ms = ?
         WHERE id = ? AND (metadata_source_id IS NOT ? OR release_id IS NOT ? OR title != ? OR artist != ? OR duration_ms != ?)",
    ).bind(&metadata.source).bind(&metadata.release_id).bind(&metadata.title).bind(&metadata.artist).bind(metadata.duration_ms)
        .bind(recording).bind(&metadata.source).bind(&metadata.release_id).bind(&metadata.title).bind(&metadata.artist).bind(metadata.duration_ms)
        .execute(&mut **tx).await?;
    Ok(())
}

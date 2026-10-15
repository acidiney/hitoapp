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
    let parent = file.path.parent().unwrap_or(root);
    let folder = match parent.to_str() {
        Some(path) => std::borrow::Cow::Borrowed(path),
        None => std::borrow::Cow::Owned(super::local_path::encode(parent)?),
    };
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
    super::metadata_owner::refresh_metadata(tx, recording).await
}

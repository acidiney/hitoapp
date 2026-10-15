use super::types::{IndexedFile, METADATA_VERSION};
use crate::{Error, Result};
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
    let observation = observe(tx, recording, source, release, file).await?;
    if let Some(audio) = &file.audio {
        sqlx::query("INSERT OR IGNORE INTO recording_evidence(recording_id, algorithm, digest) VALUES (?, ?, ?)")
            .bind(recording).bind(audio.algorithm).bind(&audio.digest).execute(&mut **tx).await?;
    }
    sqlx::query(
        "INSERT INTO source_metadata(source_id, release_id, title, artist, duration_ms, disc_number, track_number, inspection_version, observation_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(source_id) DO UPDATE SET release_id = excluded.release_id, title = excluded.title,
            artist = excluded.artist, duration_ms = excluded.duration_ms, disc_number = excluded.disc_number,
            track_number = excluded.track_number, inspection_version = excluded.inspection_version, observation_id = excluded.observation_id",
    ).bind(source).bind(release).bind(&file.title).bind(&file.artist).bind(file.duration_ms)
        .bind(file.disc_number).bind(file.track_number).bind(METADATA_VERSION).bind(observation).execute(&mut **tx).await?;
    let bound = sqlx::query(
        "INSERT INTO source_artist_credits(source_id, artist_id)
         SELECT m.source_id, c.id FROM source_metadata m JOIN sources s ON s.id = m.source_id
         JOIN recordings r ON r.id = s.recording_id
         JOIN artists c ON c.library_id = r.library_id AND c.name = m.artist COLLATE BINARY
         WHERE m.source_id = ? AND r.id = ?
         ON CONFLICT(source_id) DO UPDATE SET artist_id = excluded.artist_id",
    )
    .bind(source)
    .bind(recording)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if bound != 1 {
        return Err(Error::Internal(
            "Source artist credit could not be recorded".into(),
        ));
    }
    super::metadata_owner::refresh_metadata(tx, recording).await
}

#[tracing::instrument(level = "trace", skip_all, err(Debug))]
async fn observe(
    tx: &mut Transaction<'_, Sqlite>,
    recording: &str,
    source: &str,
    release: &str,
    file: &IndexedFile,
) -> Result<String> {
    sqlx::query(
        "INSERT INTO metadata_observations(id, source_id, recording_id, release_id, provider, origin,
            observed_at, file_checksum, inspection_version, title, artist, album, album_artist,
            duration_ms, disc_number, track_number, tags)
         SELECT ?, s.id, ?, ?, s.provider, 'file_inspection', unixepoch(), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
         FROM sources s WHERE s.id = ?
         ON CONFLICT(source_id, file_checksum, inspection_version) DO NOTHING",
    ).bind(Uuid::new_v4().to_string()).bind(recording).bind(release).bind(&file.checksum)
        .bind(METADATA_VERSION).bind(&file.title).bind(&file.artist).bind(&file.album).bind(&file.album_artist)
        .bind(file.duration_ms).bind(file.disc_number).bind(file.track_number).bind(&file.tag_details).bind(source)
        .execute(&mut **tx).await?;
    Ok(sqlx::query_scalar("SELECT id FROM metadata_observations WHERE source_id = ? AND file_checksum = ? AND inspection_version = ?")
        .bind(source).bind(&file.checksum).bind(METADATA_VERSION).fetch_one(&mut **tx).await?)
}

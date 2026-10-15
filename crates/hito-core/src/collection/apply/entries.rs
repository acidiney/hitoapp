use super::{Transaction, playlists};
use crate::{Actor, Error, Result, identity::recording_access};
use uuid::Uuid;

pub(super) async fn insert(
    tx: &mut Transaction<'_>,
    actor: &Actor,
    playlist: &Uuid,
    entry: &Uuid,
    recording: &Uuid,
    position: i64,
) -> Result<()> {
    if !playlists::editable(tx, actor, playlist).await? {
        return Ok(());
    }
    recording_access(tx, actor, &recording.to_string()).await?;
    let previous: Option<(Option<String>, bool)> = sqlx::query_as(
        "SELECT recording_id, deleted FROM playlist_entries WHERE playlist_id = ? AND id = ?",
    )
    .bind(playlist.to_string())
    .bind(entry.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((Some(previous), false)) = previous
        && previous != recording.to_string()
    {
        return Err(Error::Conflict(
            "Playlist entry IDs cannot be reused for a different recording".into(),
        ));
    }
    sqlx::query(
        "INSERT OR IGNORE INTO playlist_entries(playlist_id, id, recording_id, position)
         VALUES (?, ?, ?, ?)",
    )
    .bind(playlist.to_string())
    .bind(entry.to_string())
    .bind(recording.to_string())
    .bind(position)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(super) async fn remove(
    tx: &mut Transaction<'_>,
    actor: &Actor,
    playlist: &Uuid,
    entry: &Uuid,
) -> Result<()> {
    if !playlists::editable(tx, actor, playlist).await? {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO playlist_entries(playlist_id, id, deleted) VALUES (?, ?, 1)
         ON CONFLICT(playlist_id, id) DO UPDATE SET deleted = 1",
    )
    .bind(playlist.to_string())
    .bind(entry.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(super) async fn move_entry(
    tx: &mut Transaction<'_>,
    actor: &Actor,
    playlist: &Uuid,
    entry: &Uuid,
    position: i64,
) -> Result<()> {
    if !playlists::editable(tx, actor, playlist).await? {
        return Ok(());
    }
    sqlx::query(
        "UPDATE playlist_entries SET position = ?
         WHERE playlist_id = ? AND id = ? AND deleted = 0",
    )
    .bind(position)
    .bind(playlist.to_string())
    .bind(entry.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

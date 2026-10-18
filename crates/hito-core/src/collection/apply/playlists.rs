use super::Transaction;
use crate::{Actor, Error, Result, collection::follow::history};
use uuid::Uuid;

pub(super) async fn create(
    tx: &mut Transaction<'_>,
    actor: &Actor,
    playlist: &Uuid,
    name: &str,
) -> Result<()> {
    validate_name(name)?;
    check_owner(tx, actor, playlist).await?;
    sqlx::query("INSERT OR IGNORE INTO playlists(id, user_id, name) VALUES (?, ?, ?)")
        .bind(playlist.to_string())
        .bind(actor.id())
        .bind(name.trim())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(super) async fn rename(
    tx: &mut Transaction<'_>,
    actor: &Actor,
    playlist: &Uuid,
    name: &str,
) -> Result<()> {
    validate_name(name)?;
    check_owner(tx, actor, playlist).await?;
    if !editable(tx, actor, playlist).await? {
        return Ok(());
    }
    let affected = sqlx::query(
        "UPDATE playlists SET name = CASE WHEN deleted = 0 THEN ? ELSE name END
         WHERE id = ? AND user_id = ?",
    )
    .bind(name.trim())
    .bind(playlist.to_string())
    .bind(actor.id())
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

pub(super) async fn delete(tx: &mut Transaction<'_>, actor: &Actor, playlist: &Uuid) -> Result<()> {
    check_owner(tx, actor, playlist).await?;
    let changed = sqlx::query(
        "INSERT INTO playlists(id, user_id, name, deleted) VALUES (?, ?, '', 1)
         ON CONFLICT(id) DO UPDATE SET deleted = 1 WHERE playlists.deleted=0",
    )
    .bind(playlist.to_string())
    .bind(actor.id())
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if changed > 0 {
        crate::downloads::playlist_changed_on(tx, playlist).await?;
    }
    let deactivated = sqlx::query("UPDATE playlist_follows SET active = 0,generation = generation + 1 WHERE playlist_id = ? AND user_id = ? AND active = 1")
        .bind(playlist.to_string()).bind(actor.id()).execute(&mut **tx).await?.rows_affected();
    if deactivated > 0 {
        let cancelled = sqlx::query("UPDATE playlist_follow_jobs SET status = 'cancelled',error = 'follow_removed',updated_at = unixepoch()
            WHERE playlist_id = ? AND user_id = ? AND status = 'queued'")
            .bind(playlist.to_string()).bind(actor.id()).execute(&mut **tx).await?.rows_affected();
        let pruned = history::prune(tx, &playlist.to_string()).await?;
        tracing::debug!(
            deactivated,
            cancelled_jobs = cancelled,
            pruned_jobs = pruned,
            "Deleted playlist follow deactivated"
        );
    }
    Ok(())
}

async fn check_owner(tx: &mut Transaction<'_>, actor: &Actor, playlist: &Uuid) -> Result<()> {
    let owner: Option<String> = sqlx::query_scalar("SELECT user_id FROM playlists WHERE id = ?")
        .bind(playlist.to_string())
        .fetch_optional(&mut **tx)
        .await?;
    if owner.is_some_and(|owner| owner != actor.id()) {
        return Err(Error::NotFound);
    }
    Ok(())
}

pub(in crate::collection) async fn editable(
    tx: &mut Transaction<'_>,
    actor: &Actor,
    playlist: &Uuid,
) -> Result<bool> {
    let row: Option<(bool, String)> =
        sqlx::query_as("SELECT deleted, kind FROM playlists WHERE id = ? AND user_id = ?")
            .bind(playlist.to_string())
            .bind(actor.id())
            .fetch_optional(&mut **tx)
            .await?;
    match row {
        Some((true, _)) => Ok(false),
        Some((false, kind)) if kind == "local" => Ok(true),
        Some(_) => Err(Error::Conflict(
            "Followed playlists are read-only. Import a copy to edit.".into(),
        )),
        None => Err(Error::NotFound),
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 256 {
        Err(Error::Invalid(
            "Playlist names must contain 1 to 256 bytes".into(),
        ))
    } else {
        Ok(())
    }
}

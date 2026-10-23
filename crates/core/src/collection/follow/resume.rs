use super::history;
use crate::{Actor, Error, Result};
use sqlx::{Sqlite, Transaction};
use std::time::Instant;
use uuid::Uuid;

#[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), playlist_id = %playlist))]
pub(in crate::collection) async fn resume(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    playlist: &Uuid,
    expected_generation: i64,
    installation_revision: i64,
    service_revision: i64,
) -> Result<()> {
    let started = Instant::now();
    let playlist = playlist.to_string();
    let (deleted, generation, active): (bool, Option<i64>, Option<bool>) = sqlx::query_as(
        "SELECT p.deleted,f.generation,f.active FROM playlists p
         LEFT JOIN playlist_follows f ON f.playlist_id = p.id AND f.user_id = p.user_id
         WHERE p.id = ? AND p.user_id = ?",
    )
    .bind(&playlist)
    .bind(actor.id())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(Error::NotFound)?;
    if deleted {
        tracing::debug!(
            outcome = "deleted",
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Playlist follow resume ignored"
        );
        return Ok(());
    }
    let generation = generation
        .filter(|_| active == Some(true))
        .ok_or(Error::NotFound)?;
    if expected_generation <= 0 || installation_revision <= 0 || service_revision <= 0 {
        return Err(Error::Invalid(
            "Use positive playlist generation and connection revisions".into(),
        ));
    }
    if generation != expected_generation {
        return Err(Error::Conflict(
            "This playlist changed. Review its connection again.".into(),
        ));
    }
    let authorized: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM current_playlist_follow_authority WHERE playlist_id = ?)",
    )
    .bind(&playlist)
    .fetch_one(&mut **tx)
    .await?;
    if authorized {
        return Err(Error::Conflict(
            "This playlist connection is already approved for updates".into(),
        ));
    }
    let reviewed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playlist_follows f
        JOIN playlists p ON p.id = f.playlist_id AND p.user_id = f.user_id AND p.deleted = 0 AND p.kind = 'followed'
        JOIN users account ON account.id = f.user_id
        JOIN plugin_installations installation ON installation.id = f.installation_id
            AND installation.user_id = f.user_id AND installation.active = 1
            AND installation.revision = ? AND installation.service_id = f.service_id
            AND json_extract(installation.manifest,'$.id') = f.provider
        JOIN plugin_services service ON service.id = f.service_id AND service.active = 1 AND service.revision = ?
        WHERE f.playlist_id = ? AND f.user_id = ? AND f.active = 1 AND f.generation = ?
            AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'playlists'))")
        .bind(installation_revision).bind(service_revision).bind(&playlist).bind(actor.id()).bind(generation)
        .fetch_one(&mut **tx).await?;
    if !reviewed {
        return Err(Error::Conflict(
            "The reviewed playlist connection changed. Review it again.".into(),
        ));
    }
    let next_generation = generation.checked_add(1).ok_or_else(|| {
        Error::Conflict("This playlist cannot advance its update generation".into())
    })?;
    let changed = sqlx::query("UPDATE playlist_follows SET installation_revision = ?,service_revision = ?,generation = ?,error = NULL,next_refresh_at = unixepoch()
        WHERE playlist_id = ? AND user_id = ? AND generation = ? AND active = 1")
        .bind(installation_revision).bind(service_revision).bind(next_generation)
        .bind(&playlist).bind(actor.id()).bind(generation).execute(&mut **tx).await?.rows_affected();
    if changed != 1 {
        return Err(Error::Conflict(
            "This playlist changed. Review its connection again.".into(),
        ));
    }
    let cancelled = sqlx::query("UPDATE playlist_follow_jobs SET status = 'cancelled',error = 'approval_changed',updated_at = unixepoch()
        WHERE playlist_id = ? AND user_id = ? AND generation = ? AND status = 'queued'")
        .bind(&playlist).bind(actor.id()).bind(generation).execute(&mut **tx).await?.rows_affected();
    let pruned = history::prune(tx, &playlist).await?;
    tracing::info!(
        outcome = "resumed",
        generation = next_generation,
        cancelled_jobs = cancelled,
        pruned_jobs = pruned,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Playlist follow execution approved"
    );
    Ok(())
}

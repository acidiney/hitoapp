use super::{
    Collection, PlaylistFollow, PlaylistFollowInterval, PlaylistFollowRefresh,
    PlaylistFollowStatus, RefreshFollowedPlaylist, receipts,
};
use crate::{Actor, Error, Result};
use sqlx::{Row, SqliteConnection};
use uuid::Uuid;

pub(super) const MAX_ACCOUNT_PENDING: i64 = 16;
pub(super) const MAX_PENDING: i64 = 128;

impl Collection {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn enqueue_follow_refresh(
        &self,
        actor: &Actor,
        playlist: &str,
        request: RefreshFollowedPlaylist,
    ) -> Result<PlaylistFollowRefresh> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        receipts::expire_refresh_receipts(&mut tx).await?;
        if let Some(receipt) =
            receipts::refresh(&mut tx, actor.id(), playlist, request.request_id).await?
        {
            tx.commit().await?;
            return Ok(receipt);
        }
        let generation: i64 = sqlx::query_scalar("SELECT f.generation FROM playlist_follows f JOIN playlists p ON p.id = f.playlist_id AND p.deleted = 0
            WHERE f.playlist_id = ? AND f.user_id = ? AND f.active = 1")
            .bind(playlist).bind(actor.id()).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        check_authority(&mut tx, playlist).await?;
        receipts::check_refresh_capacity(&mut tx, actor.id()).await?;
        let pending: Option<(String, i64)> = sqlx::query_as("SELECT id,generation FROM playlist_follow_jobs WHERE playlist_id = ? AND status IN ('queued','running')")
            .bind(playlist).fetch_optional(&mut *tx).await?;
        let job = if let Some((id, pending_generation)) = pending {
            if pending_generation != generation {
                tracing::debug!(
                    outcome = "previous_generation_pending",
                    "Playlist refresh request deferred"
                );
                return Err(Error::Conflict(
                    "A previous playlist update is still finishing. Try again.".into(),
                ));
            }
            id
        } else {
            check_capacity(&mut tx, actor.id()).await?;
            let id = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO playlist_follow_jobs(id,user_id,playlist_id,generation,status) VALUES (?,?,?,?,'queued')")
                .bind(&id).bind(actor.id()).bind(playlist).bind(generation).execute(&mut *tx).await?;
            id
        };
        sqlx::query("INSERT INTO playlist_follow_refresh_receipts(user_id,request_id,playlist_id,job_id) VALUES (?,?,?,?)")
            .bind(actor.id()).bind(request.request_id.to_string()).bind(playlist).bind(&job).execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::info!(job_id = %job, outcome = "queued", "Follow refresh requested");
        Ok(PlaylistFollowRefresh {
            request_id: request.request_id,
            job_id: receipts::identity(&job)?,
            playlist_id: receipts::identity(playlist)?,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn playlist_follow(&self, actor: &Actor, playlist: &str) -> Result<PlaylistFollow> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let row = sqlx::query("SELECT f.interval_seconds,f.snapshot,f.last_refreshed_at,f.next_refresh_at,f.error,
            f.generation,f.content_revision,
            EXISTS(SELECT 1 FROM current_playlist_follow_authority authority WHERE authority.playlist_id = f.playlist_id) AS authorized,
            (SELECT status FROM playlist_follow_jobs job WHERE job.playlist_id = f.playlist_id AND job.generation = f.generation ORDER BY job.created_at DESC,job.rowid DESC LIMIT 1) AS job_status
            FROM playlist_follows f JOIN playlists p ON p.id = f.playlist_id AND p.user_id = f.user_id AND p.deleted = 0
            WHERE f.playlist_id = ? AND f.user_id = ? AND f.active = 1")
            .bind(playlist).bind(actor.id()).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        let authorized: bool = row.try_get("authorized")?;
        let job_status: Option<String> = row.try_get("job_status")?;
        let status = if !authorized {
            PlaylistFollowStatus::Paused
        } else {
            match job_status.as_deref() {
                Some("queued") => PlaylistFollowStatus::Queued,
                Some("running") => PlaylistFollowStatus::Running,
                Some("failed") => PlaylistFollowStatus::Failed,
                _ => PlaylistFollowStatus::Ready,
            }
        };
        let result = PlaylistFollow {
            playlist_id: receipts::identity(playlist)?,
            generation: row.try_get("generation")?,
            content_revision: row.try_get("content_revision")?,
            interval: PlaylistFollowInterval::from_seconds(row.try_get("interval_seconds")?)?,
            snapshot: row.try_get("snapshot")?,
            last_refreshed_at: row.try_get("last_refreshed_at")?,
            next_refresh_at: row.try_get("next_refresh_at")?,
            status,
            error: if authorized {
                row.try_get("error")?
            } else {
                Some("authority_changed".into())
            },
        };
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        Ok(result)
    }
}

pub(super) async fn check_authority(
    connection: &mut SqliteConnection,
    playlist: &str,
) -> Result<()> {
    let current: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM current_playlist_follow_authority WHERE playlist_id = ?)",
    )
    .bind(playlist)
    .fetch_one(connection)
    .await?;
    if !current {
        return Err(Error::Conflict(
            "This followed playlist connection changed".into(),
        ));
    }
    Ok(())
}

pub(super) async fn check_capacity(connection: &mut SqliteConnection, account: &str) -> Result<()> {
    let (account_count, total): (i64, i64) = sqlx::query_as("SELECT coalesce(sum(user_id = ?),0),count(*) FROM playlist_follow_jobs WHERE status IN ('queued','running')")
        .bind(account).fetch_one(connection).await?;
    if account_count >= MAX_ACCOUNT_PENDING || total >= MAX_PENDING {
        return Err(Error::Conflict(
            "Playlist refresh work is busy. Try again.".into(),
        ));
    }
    Ok(())
}

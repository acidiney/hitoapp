use super::{Collection, Playlist, PlaylistDetail, follow, import_persist, playlist_entries};
use crate::{Actor, Error, Result};

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn playlists(&self, actor: &Actor) -> Result<Vec<Playlist>> {
        Ok(sqlx::query_as(
            "SELECT id, name, kind FROM playlists
             WHERE user_id = ? AND deleted = 0 ORDER BY name, id LIMIT 1000",
        )
        .bind(actor.id())
        .fetch_all(&self.store.pool)
        .await?)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn playlist(&self, actor: &Actor, id: &str) -> Result<PlaylistDetail> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let playlist = sqlx::query_as(
            "SELECT id, name, kind FROM playlists
             WHERE id = ? AND user_id = ? AND deleted = 0",
        )
        .bind(id)
        .bind(actor.id())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        let entries = playlist_entries::read(&mut tx, &self.host, actor, id).await?;
        let origin = if let Some(origin) = import_persist::origin(&mut tx, id).await? {
            Some(origin)
        } else {
            follow::origin(&mut tx, id).await?
        };
        let follow_content_revision = sqlx::query_scalar(
            "SELECT content_revision FROM playlist_follows WHERE playlist_id = ? AND user_id = ?",
        )
        .bind(id)
        .bind(actor.id())
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        Ok(PlaylistDetail {
            playlist,
            entries,
            origin,
            follow_content_revision,
        })
    }
}

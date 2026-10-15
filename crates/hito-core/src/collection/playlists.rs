use super::{Collection, Playlist, PlaylistDetail};
use crate::{Actor, Error, Result, catalog::push_available, metadata};
use sqlx::QueryBuilder;

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
        let playlist = sqlx::query_as(
            "SELECT id, name, kind FROM playlists
             WHERE id = ? AND user_id = ? AND deleted = 0",
        )
        .bind(id)
        .bind(actor.id())
        .fetch_optional(&self.store.pool)
        .await?
        .ok_or(Error::NotFound)?;
        let mut query = QueryBuilder::new(
            "SELECT e.id AS entry_id, e.recording_id, e.position,
                    COALESCE(k.liked, 0) AS liked,
                    CASE WHEN m.user_id IS NULL THEN 'Unavailable recording' ELSE COALESCE(personal_title.override_title, r.title) END AS title,
                    CASE WHEN m.user_id IS NULL THEN '' ELSE r.artist END AS artist,
                    CASE WHEN m.user_id IS NULL THEN 0 ELSE r.duration_ms END AS duration_ms,
                    (m.user_id IS NOT NULL AND ",
        );
        push_available(&mut query, &self.host);
        query
            .push(
                ") AS available
             FROM playlist_entries e
             LEFT JOIN recordings r ON r.id = e.recording_id
             LEFT JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ",
            )
            .push_bind(actor.id().to_owned())
            .push(
                " LEFT JOIN likes k ON k.recording_id = r.id AND k.user_id = m.user_id
              ",
            );
        metadata::join_title(&mut query, actor);
        query
            .push(" WHERE e.playlist_id = ")
            .push_bind(id.to_owned())
            .push(" AND e.deleted = 0 ORDER BY e.position, e.id LIMIT 10000");
        let entries = query.build_query_as().fetch_all(&self.store.pool).await?;
        Ok(PlaylistDetail { playlist, entries })
    }
}

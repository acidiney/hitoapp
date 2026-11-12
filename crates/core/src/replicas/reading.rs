use super::{ReplicaSession, Replicas, files};
use crate::{Actor, Browse, Error, Page, Playlist, Result};

impl Replicas {
    #[tracing::instrument(level="debug",skip_all,err(Debug),fields(account_id=actor.id(),connection_id=%session.connection))]
    pub async fn playlists(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        query: Browse,
    ) -> Result<Page<Playlist>> {
        if query.q.is_some()
            || query.library.is_some()
            || query.artist.is_some()
            || query.sort.is_some()
            || query.recording_offset.is_some()
            || query.featured_offset.is_some()
        {
            return Err(Error::Invalid(
                "Replica playlists support offset and limit only".into(),
            ));
        }
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(60);
        if offset < 0 || !(1..=200).contains(&limit) {
            return Err(Error::Invalid(
                "Use a nonnegative offset and a page size from 1 to 200".into(),
            ));
        }
        self.guarded(actor, session).await?.commit().await?;
        let profile = self
            .profiles
            .active
            .lock()
            .map_err(|_| files::unavailable())?
            .get(&session.connection)
            .cloned()
            .ok_or(Error::NotFound)?;
        let mut tx = profile.store.pool.begin().await?;
        let total: i64 =
            sqlx::query_scalar("SELECT count(*) FROM playlists WHERE user_id=? AND deleted=0")
                .bind(&session.remote_account)
                .fetch_one(&mut *tx)
                .await?;
        let items=sqlx::query_as("SELECT id,name,kind FROM playlists WHERE user_id=? AND deleted=0 ORDER BY name,id LIMIT ? OFFSET ?")
            .bind(&session.remote_account).bind(limit).bind(offset).fetch_all(&mut *tx).await?;
        let guard = self.guarded(actor, session).await?;
        tx.commit().await?;
        drop(profile);
        guard.commit().await?;
        tracing::debug!(outcome = "read", count = total, "Replica playlists read");
        Ok(Page {
            items,
            total,
            offset,
            limit,
        })
    }
}

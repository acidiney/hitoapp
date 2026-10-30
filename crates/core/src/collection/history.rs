use super::{Activity, Collection};
use crate::{
    Actor, Result,
    catalog::{push_available, push_plays_from_services},
    metadata,
};
use sqlx::QueryBuilder;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn activity(&self, actor: &Actor) -> Result<Activity> {
        let mut query = QueryBuilder::new("SELECT r.id, ");
        query
            .push(metadata::RECORDING_TITLE)
            .push(" AS title, r.title AS inherited_title, ").push(metadata::RECORDING_ARTIST)
            .push(" AS artist, r.artist AS inherited_artist, r.duration_ms, p.position_ms, p.speed, p.updated_at, ");
        push_available(&mut query, &self.host, actor.id());
        query.push(" AS available, ");
        push_plays_from_services(&mut query, actor.id());
        query.push(
            " AS plays_from_services, COALESCE(l.liked, 0) AS liked
              FROM listening_progress p
              JOIN recordings r ON p.recording_id = r.id
              JOIN library_members m ON m.library_id = r.library_id AND m.user_id = p.user_id
              LEFT JOIN likes l ON l.recording_id = r.id AND l.user_id = p.user_id
              ",
        );
        metadata::join_recording_fields(&mut query, actor);
        query
            .push(" WHERE p.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" ORDER BY p.updated_at DESC, r.id LIMIT 200");
        let history = query.build_query_as().fetch_all(&self.store.pool).await?;
        Ok(Activity { history })
    }
}

use super::{Catalog, push_source_choice};
use crate::{Actor, Result};
use sqlx::QueryBuilder;

#[derive(Clone, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct PodcastEnclosure {
    pub source_id: String,
    pub episode_id: String,
    pub recording_id: String,
    pub user_id: String,
    pub subscription_id: String,
    pub subscription_generation: i64,
    pub enclosure_revision: i64,
    pub enclosure_url: String,
    pub declared_mime: Option<String>,
    pub byte_length: Option<i64>,
}

impl Catalog {
    pub(super) async fn podcast_enclosure(
        &self,
        actor: &Actor,
        source: &str,
    ) -> Result<Option<PodcastEnclosure>> {
        let mut query = QueryBuilder::new(
            "SELECT enclosure.source_id,enclosure.episode_id,enclosure.recording_id,enclosure.user_id,
                enclosure.subscription_id,enclosure.subscription_generation,enclosure.enclosure_revision,
                enclosure.enclosure_url,enclosure.declared_mime,enclosure.byte_length
            FROM current_podcast_sources enclosure CROSS JOIN sources s ON s.id=enclosure.source_id
            WHERE enclosure.source_id=",
        );
        query
            .push_bind(source.to_owned())
            .push(" AND enclosure.user_id=")
            .push_bind(actor.id().to_owned())
            .push(" AND ");
        push_source_choice(&mut query, actor.id());
        Ok(query
            .build_query_as()
            .fetch_optional(&self.store.pool)
            .await?)
    }
}

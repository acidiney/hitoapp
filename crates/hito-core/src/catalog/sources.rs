use super::{Catalog, Source, SourceAccess, push_source_access};
use crate::{Actor, Result, identity::recording_access};
use sqlx::QueryBuilder;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn sources(&self, actor: &Actor, recording: &str) -> Result<Vec<Source>> {
        let mut connection = self.store.pool.acquire().await?;
        recording_access(&mut connection, actor, recording).await?;
        let mut query =
            QueryBuilder::new("SELECT s.id, s.recording_id, s.provider, s.device_id, (");
        push_source_access(&mut query, &self.host, SourceAccess::Playable);
        query
            .push(") AS ready FROM sources s WHERE s.recording_id = ")
            .push_bind(recording.to_owned())
            .push(" ORDER BY CASE s.provider WHEN 'local' THEN 0 ELSE 1 END, ready DESC, s.id");
        Ok(query.build_query_as().fetch_all(&mut *connection).await?)
    }
}

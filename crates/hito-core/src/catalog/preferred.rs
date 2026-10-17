use super::{Catalog, PlaybackSource, SourceAccess, push_source_access};
use crate::{Actor, Error, Result, identity::recording_access};
use sqlx::QueryBuilder;
use std::time::Duration;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn preferred_playback_source(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<PlaybackSource> {
        tokio::time::timeout(Duration::from_secs(1), async {
            actor.check_session(&self.store).await?;
            let mut connection = self.store.pool.acquire().await?;
            recording_access(&mut connection, actor, recording).await?;
            let mut local = QueryBuilder::new(
                "SELECT s.id FROM sources s INDEXED BY sources_metadata_owner
                WHERE s.recording_id = ",
            );
            local
                .push_bind(recording.to_owned())
                .push(" AND s.provider IN ('local','download') AND ");
            push_source_access(&mut local, &self.host, actor.id(), SourceAccess::LocalFile);
            local.push(" ORDER BY CASE s.provider WHEN 'local' THEN 0 ELSE 1 END, s.id LIMIT 1");
            let source: Option<String> = local
                .build_query_scalar()
                .fetch_optional(&mut *connection)
                .await?;
            let source = match source {
                Some(source) => source,
                None => {
                    let mut provider = QueryBuilder::new(
                        "WITH candidates(id,priority) AS (
                        SELECT source_id,0 FROM current_podcast_sources WHERE recording_id=",
                    );
                    provider.push_bind(recording.to_owned()).push(" AND user_id=")
                        .push_bind(actor.id().to_owned()).push(" UNION ALL
                        SELECT ps.source_id,1 FROM provider_items item
                        CROSS JOIN provider_source_state state ON state.binding_id = item.id
                        CROSS JOIN provider_sources ps INDEXED BY provider_sources_generation
                            ON ps.binding_id = state.binding_id AND ps.generation = state.generation
                        WHERE item.retired = 0 AND item.recording_id = ");
                    provider
                        .push_bind(recording.to_owned())
                        .push(") SELECT s.id FROM candidates candidate CROSS JOIN sources s ON s.id=candidate.id
                            WHERE s.provider IN ('podcast','plugin') AND ");
                    push_source_access(
                        &mut provider,
                        &self.host,
                        actor.id(),
                        SourceAccess::Playable,
                    );
                    provider.push(" ORDER BY candidate.priority,s.id LIMIT 1");
                    provider
                        .build_query_scalar()
                        .fetch_optional(&mut *connection)
                        .await?
                        .ok_or(Error::NotFound)?
                }
            };
            drop(connection);
            self.playback_source(actor, recording, &source).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback source selection could not finish in time",
        )))
    }
}

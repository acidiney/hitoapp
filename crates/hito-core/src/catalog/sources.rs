use super::{Catalog, Source, SourceAccess, push_source_access};
use crate::{Actor, Result, identity::recording_access};
use sqlx::QueryBuilder;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn sources(&self, actor: &Actor, recording: &str) -> Result<Vec<Source>> {
        let mut connection = self.store.pool.acquire().await?;
        recording_access(&mut connection, actor, recording).await?;
        let mut query = QueryBuilder::new(
            "WITH selected(id,label,preparation_approved) AS (
            SELECT id,CASE provider WHEN 'local' THEN 'Local audio' WHEN 'download' THEN 'Saved audio' ELSE 'Audio' END,0
            FROM sources INDEXED BY nonplugin_sources_recording WHERE provider != 'plugin' AND recording_id = ",
        );
        query
            .push_bind(recording.to_owned())
            .push(" AND (provider != 'download' OR EXISTS (SELECT 1 FROM download_files file
                WHERE file.source_id = sources.id AND file.account_id = ")
            .push_bind(actor.id().to_owned())
            .push(" AND file.state = 'ready'))")
            .push(
                " UNION ALL
            SELECT ps.source_id,ps.label,(installation.active = 1 AND service.active = 1
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'sources')
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'source_preparation'))
            FROM provider_items item CROSS JOIN provider_source_state state ON state.binding_id = item.id
            CROSS JOIN provider_sources ps INDEXED BY provider_sources_generation
            ON ps.binding_id = state.binding_id AND ps.generation = state.generation
                AND ps.installation_id = item.installation_id AND ps.recording_id = item.recording_id
            JOIN plugin_installations installation ON installation.id = item.installation_id
            JOIN plugin_services service ON service.id = installation.service_id
            WHERE item.retired = 0 AND installation.user_id = ",
            )
            .push_bind(actor.id().to_owned())
            .push(" AND item.recording_id = ")
            .push_bind(recording.to_owned())
            .push(
                ")
            SELECT s.id, s.recording_id, s.provider, ids.label, s.device_id,
                (ids.preparation_approved AND s.ready = 0) AS can_prepare, (",
            );
        push_source_access(&mut query, &self.host, actor.id(), SourceAccess::Playable);
        query
            .push(") AS ready FROM selected ids CROSS JOIN sources s ON s.id = ids.id")
            .push(" ORDER BY CASE s.provider WHEN 'local' THEN 0 WHEN 'download' THEN 1 ELSE 2 END, ready DESC, s.id");
        Ok(query.build_query_as().fetch_all(&mut *connection).await?)
    }
}

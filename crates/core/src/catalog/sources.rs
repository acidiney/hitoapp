use super::provider::FALLBACK_POLICY;
use super::{
    Catalog, Source, SourceAccess, SourceChoiceEntry, push_source_access,
    source_choice::{push_selectable_source, push_selected_source},
};
use crate::{Actor, HostDeviceId, Result, identity::recording_access};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn sources(&self, actor: &Actor, recording: &str) -> Result<Vec<Source>> {
        let mut connection = self.store.pool.acquire().await?;
        recording_access(&mut connection, actor, recording).await?;
        let mut query = source_query(&self.host, actor.id(), recording, false);
        Ok(query.build_query_as().fetch_all(&mut *connection).await?)
    }

    pub(super) async fn source_entries_on(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
    ) -> Result<Vec<SourceChoiceEntry>> {
        let mut query = source_query(&self.host, actor.id(), recording, true);
        Ok(query.build_query_as().fetch_all(connection).await?)
    }
}

fn source_query(
    host: &HostDeviceId,
    account: &str,
    recording: &str,
    controls: bool,
) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new(
            "WITH selected(id,label,preparation_approved,provider_label) AS (
            SELECT id,CASE provider WHEN 'local' THEN 'Local audio' WHEN 'download' THEN 'Saved audio' WHEN 'podcast' THEN 'Podcast audio' ELSE 'Audio' END,0,NULL
            FROM sources INDEXED BY nonplugin_sources_recording WHERE provider != 'plugin' AND recording_id = ",
        );
    query
            .push_bind(recording.to_owned())
            .push(" AND (provider != 'download' OR EXISTS (SELECT 1 FROM download_files file
                WHERE file.source_id = sources.id AND file.account_id = ")
            .push_bind(account.to_owned())
            .push(" AND file.state = 'ready'))")
            .push(
                " UNION ALL
            SELECT ps.source_id,ps.label,(installation.active = 1 AND service.active = 1
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'sources')
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'source_preparation')),
                service.name
            FROM provider_items item CROSS JOIN provider_source_state state ON state.binding_id = item.id
            CROSS JOIN provider_sources ps INDEXED BY provider_sources_generation
            ON ps.binding_id = state.binding_id AND ps.generation = state.generation
                AND ps.installation_id = item.installation_id AND ps.recording_id = item.recording_id
            JOIN plugin_installations installation ON installation.id = item.installation_id
            JOIN plugin_services service ON service.id = installation.service_id
            WHERE item.retired = 0 AND installation.user_id = ",
            )
            .push_bind(account.to_owned())
            .push(" AND item.recording_id = ")
            .push_bind(recording.to_owned())
            .push(
                ")
            SELECT s.id, s.recording_id, s.provider, ids.label, ids.provider_label, s.device_id,
                (ids.preparation_approved AND s.ready = 0) AS can_prepare, (",
            );
    push_source_access(&mut query, host, account, SourceAccess::Playable);
    query.push(") AS ready, CASE WHEN s.provider = 'plugin' AND EXISTS(
        SELECT 1 FROM provider_sources remote
        JOIN provider_items item ON item.id = remote.binding_id AND item.retired = 0
            AND ((item.evidence = 'automatic_policy' AND item.role = 'source')
                OR (item.evidence = 'provider_identity' AND item.role = 'primary'
                    AND item.id = (SELECT metadata_binding_id FROM recordings WHERE id = s.recording_id)))
        WHERE remote.source_id = s.id AND remote.recording_id = s.recording_id
            AND NOT EXISTS(SELECT 1 FROM recording_source_choices choice
                WHERE choice.user_id = ")
        .push_bind(account.to_owned())
        .push(" AND choice.recording_id = s.recording_id AND choice.binding_id = item.id)
            AND (SELECT decision.policy_version FROM automatic_source_decisions decision
                WHERE decision.binding_id = item.id ORDER BY decision.rowid DESC LIMIT 1) = ")
        .push_bind(FALLBACK_POLICY)
        .push(") THEN 'best_guess' ELSE 'verified' END AS match_kind");
    if controls {
        query.push(", (");
        push_selected_source(&mut query, host, account);
        query.push(") AS selected, (");
        push_selectable_source(&mut query, account);
        query.push(") AS selectable");
    }
    query
            .push(" FROM selected ids CROSS JOIN sources s ON s.id = ids.id")
            .push(" ORDER BY CASE s.provider WHEN 'local' THEN 0 WHEN 'download' THEN 1 ELSE 2 END, ready DESC, s.id");
    query
}

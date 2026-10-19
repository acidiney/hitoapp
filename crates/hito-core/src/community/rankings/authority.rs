use super::super::{
    CommunityRankingPreferences, ranking_preferences,
    source::{PublicSource, reference},
};
use crate::{
    Actor, HostDeviceId, Result, Source,
    catalog::{SourceAccess, push_source_access},
    identity::recording_access,
    store::Store,
};
use sqlx::{QueryBuilder, SqliteConnection};
use std::cmp::Reverse;

const MAX_SOURCES: usize =
    crate::catalog::MAX_PLAYBACK_BINDINGS * hito_plugin_sdk::MAX_SOURCES_PER_RECORDING;

#[derive(PartialEq, Eq, sqlx::FromRow)]
struct Identity {
    library_id: String,
    access_grant: String,
    metadata_binding_id: Option<String>,
    metadata_observation_id: Option<String>,
    choice_revision: i64,
    choice_binding_id: Option<String>,
    protected: bool,
}

#[derive(PartialEq, Eq, sqlx::FromRow)]
struct Witness {
    source_id: String,
    binding_id: String,
    binding_revision: i64,
    evidence: String,
    observation_id: String,
    generation: i64,
    installation_id: String,
    installation_revision: i64,
    service_id: String,
    service_revision: i64,
    provider: String,
    provider_recording_id: String,
}

#[derive(PartialEq, Eq)]
pub(super) struct Snapshot {
    identity: Identity,
    preferences: CommunityRankingPreferences,
    witnesses: Vec<Witness>,
    pub(super) candidates: Vec<PublicSource>,
    sources: Vec<(String, usize)>,
}

impl Snapshot {
    pub(super) async fn capture(
        store: &Store,
        host: &HostDeviceId,
        actor: &Actor,
        recording: &str,
        sources: &[Source],
    ) -> Result<Option<Self>> {
        let mut tx = store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let snapshot = Self::capture_on(&mut tx, host, actor, recording, sources).await?;
        tx.commit().await?;
        actor.check_session(store).await?;
        Ok(snapshot)
    }

    async fn capture_on(
        connection: &mut SqliteConnection,
        host: &HostDeviceId,
        actor: &Actor,
        recording: &str,
        sources: &[Source],
    ) -> Result<Option<Self>> {
        let preferences = ranking_preferences::read(connection, actor).await?;
        if !preferences.use_source_rankings {
            return Ok(None);
        }
        let identity: Option<Identity> = sqlx::query_as("SELECT recording.library_id,access.id AS access_grant,
            recording.metadata_binding_id,recording.metadata_observation_id,
            COALESCE(choice.revision,0) AS choice_revision,choice.binding_id AS choice_binding_id,
            CASE WHEN choice.binding_id IS NOT NULL THEN 1 WHEN COALESCE(choice.revision,0)>0 THEN 0
                ELSE EXISTS(SELECT 1 FROM provider_items reviewed WHERE reviewed.recording_id=recording.id
                    AND reviewed.retired=0 AND reviewed.evidence='user_confirmed') END AS protected
            FROM recordings recording JOIN libraries library ON library.id=recording.library_id AND library.owner_id=?
            JOIN library_access_grants access ON access.library_id=recording.library_id AND access.user_id=?
            LEFT JOIN recording_source_choices choice ON choice.user_id=? AND choice.recording_id=recording.id
            WHERE recording.id=?")
            .bind(actor.id()).bind(actor.id()).bind(actor.id()).bind(recording)
            .fetch_optional(&mut *connection).await?;
        let Some(identity) = identity.filter(|identity| !identity.protected) else {
            return Ok(None);
        };
        let eligible: Vec<_> = sources
            .iter()
            .filter(|source| source.ready && source.provider == "plugin")
            .take(MAX_SOURCES + 1)
            .collect();
        if !(2..=MAX_SOURCES).contains(&eligible.len()) {
            return Ok(None);
        }
        let mut query = QueryBuilder::new("SELECT s.id AS source_id,item.id AS binding_id,item.revision AS binding_revision,
            item.evidence,state.observation_id,state.generation,installation.id AS installation_id,
            installation.revision AS installation_revision,service.id AS service_id,service.revision AS service_revision,
            json_extract(installation.manifest,'$.id') AS provider,item.provider_recording_id
            FROM provider_items item INDEXED BY provider_items_recording
            CROSS JOIN provider_source_state state ON state.binding_id=item.id
            CROSS JOIN provider_sources remote INDEXED BY provider_sources_generation
                ON remote.binding_id=item.id AND remote.generation=state.generation
                AND remote.installation_id=item.installation_id AND remote.recording_id=item.recording_id
            JOIN sources s ON s.id=remote.source_id AND s.recording_id=item.recording_id
            JOIN plugin_installations installation ON installation.id=item.installation_id AND installation.active=1
            JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
            WHERE item.retired=0 AND item.recording_id=");
        query.push_bind(recording.to_owned()).push(" AND installation.user_id=").push_bind(actor.id().to_owned())
            .push(" AND (item.role='primary' OR item.evidence IN ('automatic_policy','user_confirmed')) AND s.id IN (");
        let mut ids = query.separated(",");
        for source in eligible {
            ids.push_bind(source.id.clone());
        }
        ids.push_unseparated(") AND ");
        push_source_access(&mut query, host, actor.id(), SourceAccess::Playable);
        query
            .push(" ORDER BY s.id LIMIT ")
            .push_bind((MAX_SOURCES + 1) as i64);
        let witnesses: Vec<Witness> = query.build_query_as().fetch_all(connection).await?;
        if witnesses.len() > MAX_SOURCES {
            return Ok(None);
        }
        let mut candidates = Vec::new();
        let mut ranked_sources = Vec::new();
        for source in sources {
            let mut matching = witnesses
                .iter()
                .filter(|witness| witness.source_id == source.id);
            let Some(witness) = matching.next() else {
                continue;
            };
            if matching.next().is_some() {
                return Ok(None);
            }
            let Some(public) = reference(&witness.provider, witness.provider_recording_id.clone())
            else {
                continue;
            };
            let index = candidates
                .iter()
                .position(|candidate| candidate == &public)
                .unwrap_or_else(|| {
                    candidates.push(public);
                    candidates.len() - 1
                });
            ranked_sources.push((source.id.clone(), index));
        }
        if !(2..=super::MAX_CANDIDATES).contains(&candidates.len()) {
            return Ok(None);
        }
        Ok(Some(Self {
            identity,
            preferences,
            witnesses,
            candidates,
            sources: ranked_sources,
        }))
    }

    pub(super) fn order(mut self, scores: &[i64]) -> Vec<String> {
        self.sources
            .sort_by_key(|(_, candidate)| Reverse(scores[*candidate]));
        self.sources.into_iter().map(|(source, _)| source).collect()
    }
}

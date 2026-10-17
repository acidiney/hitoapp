use super::{Catalog, PlaybackSource, SourceAccess, push_source_access};
use crate::{Actor, Error, Result, identity::recording_access};
use sqlx::QueryBuilder;
use std::{collections::VecDeque, time::Duration};

#[derive(sqlx::FromRow)]
struct ProviderSnapshot {
    binding_id: String,
    revision: i64,
    generation: i64,
}

pub struct PlaybackCandidates {
    catalog: Catalog,
    actor: Actor,
    recording: String,
    remaining: u64,
    local: bool,
    download: bool,
    after: Option<String>,
    ceiling: Option<String>,
    providers: VecDeque<ProviderSnapshot>,
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn playback_candidates(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<PlaybackCandidates> {
        tokio::time::timeout(Duration::from_secs(1), async {
            actor.check_session(&self.store).await?;
            let mut connection = self.store.pool.acquire().await?;
            recording_access(&mut connection, actor, recording).await?;
            let mut files = QueryBuilder::new(
                "SELECT count(*), max(s.id) FROM sources s INDEXED BY sources_metadata_owner WHERE s.recording_id=",
            );
            files.push_bind(recording.to_owned()).push(" AND s.provider IN ('local','download') AND ");
            push_source_access(&mut files, &self.host, actor.id(), SourceAccess::LocalFile);
            let (count, ceiling): (i64, Option<String>) = files.build_query_as()
                .fetch_one(&mut *connection).await?;
            let providers: Vec<ProviderSnapshot> = sqlx::query_as(
                "SELECT item.id AS binding_id,item.revision,state.generation FROM provider_items item
                JOIN provider_source_state state ON state.binding_id = item.id
                JOIN plugin_installations p ON p.id = item.installation_id AND p.active = 1
                JOIN plugin_services service ON service.id = p.service_id AND service.active = 1
                WHERE item.recording_id = ? AND item.retired = 0 AND p.user_id = ?
                AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'sources')
                ORDER BY item.id LIMIT ?",
            )
            .bind(recording)
            .bind(actor.id())
            .bind(super::provider::MAX_PLAYBACK_BINDINGS as i64)
            .fetch_all(&mut *connection)
            .await?;
            Ok(PlaybackCandidates {
                catalog: self.clone(),
                actor: actor.clone(),
                recording: recording.to_owned(),
                remaining: count as u64 + (providers.len() * hito_plugin_sdk::MAX_SOURCES_PER_RECORDING) as u64,
                local: count > 0,
                download: count > 0,
                after: None,
                ceiling,
                providers: providers.into(),
            })
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback source selection could not finish in time",
        )))
    }
}

impl PlaybackCandidates {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = self.actor.id()))]
    pub async fn next(&mut self) -> Result<Option<PlaybackSource>> {
        tokio::time::timeout(Duration::from_secs(1), self.advance())
            .await
            .unwrap_or(Err(Error::Upstream(
                "Playback source selection could not finish in time",
            )))
    }

    async fn advance(&mut self) -> Result<Option<PlaybackSource>> {
        self.actor.check_session(&self.catalog.store).await?;
        let mut connection = self.catalog.store.pool.acquire().await?;
        recording_access(&mut connection, &self.actor, &self.recording).await?;
        while self.remaining > 0 {
            let mut query = if self.local || self.download {
                let mut query = QueryBuilder::new(
                    "SELECT s.id FROM sources s INDEXED BY sources_metadata_owner WHERE s.recording_id=",
                );
                query.push_bind(self.recording.clone()).push(if self.local {
                    " AND s.provider='local' AND "
                } else {
                    " AND s.provider='download' AND "
                });
                push_source_access(
                    &mut query,
                    &self.catalog.host,
                    self.actor.id(),
                    SourceAccess::LocalFile,
                );
                if let Some(ceiling) = &self.ceiling {
                    query.push(" AND s.id<=").push_bind(ceiling.clone());
                }
                query
            } else if let Some(snapshot) = self.providers.front() {
                let mut query = QueryBuilder::new(
                    "SELECT ps.source_id FROM provider_items item
                    CROSS JOIN provider_source_state state ON state.binding_id = item.id
                    CROSS JOIN provider_sources ps INDEXED BY provider_sources_generation
                    ON ps.binding_id=state.binding_id AND ps.generation=state.generation
                    CROSS JOIN sources s ON s.id=ps.source_id WHERE item.retired=0 AND item.recording_id=",
                );
                query
                    .push_bind(self.recording.clone())
                    .push(" AND item.id=")
                    .push_bind(snapshot.binding_id.clone())
                    .push(" AND item.revision=")
                    .push_bind(snapshot.revision)
                    .push(" AND state.generation=")
                    .push_bind(snapshot.generation)
                    .push(" AND s.provider='plugin' AND ");
                push_source_access(
                    &mut query,
                    &self.catalog.host,
                    self.actor.id(),
                    SourceAccess::Playable,
                );
                query
            } else {
                self.remaining = 0;
                break;
            };
            if let Some(after) = &self.after {
                query.push(" AND s.id>").push_bind(after.clone());
            }
            query.push(if self.local || self.download {
                " ORDER BY s.id LIMIT 1"
            } else {
                " ORDER BY ps.source_id LIMIT 1"
            });
            let source: Option<String> = query
                .build_query_scalar()
                .fetch_optional(&mut *connection)
                .await?;
            let Some(source) = source else {
                if self.local {
                    self.local = false;
                    self.after = None;
                    continue;
                }
                if self.download {
                    self.download = false;
                    self.after = None;
                    continue;
                }
                self.providers.pop_front();
                self.after = None;
                continue;
            };
            self.after = Some(source.clone());
            self.remaining -= 1;
            drop(connection);
            match self
                .catalog
                .playback_source(&self.actor, &self.recording, &source)
                .await
            {
                Ok(grant) => return Ok(Some(grant)),
                Err(Error::NotFound) => connection = self.catalog.store.pool.acquire().await?,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }
}

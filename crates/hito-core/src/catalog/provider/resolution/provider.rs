use super::{authority::Target, receipts::Attempt};
use crate::{Actor, Catalog, Error, Result, plugins::Operation};
use hito_plugin_sdk::{Capability, ProviderRecording, RecordingRequest, SearchRequest, SourcePage};
use sqlx::Row;

pub(super) struct Candidate {
    pub installation: String,
    pub installation_revision: i64,
    pub operation: Operation,
    pub details: ProviderRecording,
    pub sources: SourcePage,
}

impl Catalog {
    pub(super) async fn resolution_candidates(
        &self,
        actor: &Actor,
        attempt: &Attempt,
    ) -> Result<Option<Candidate>> {
        let Some(query) = attempt.target.musical.query() else {
            return Ok(None);
        };
        let connections = sqlx::query("SELECT installation.id,installation.revision FROM plugin_installations installation
            JOIN plugin_services service ON service.id = installation.service_id AND service.active = 1
            WHERE installation.user_id = ? AND installation.active = 1
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'search')
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'sources')
            ORDER BY installation.id LIMIT 4")
            .bind(actor.id()).fetch_all(&self.store.pool).await?;
        for connection in connections {
            actor.check_session(&self.store).await?;
            let installation: String = connection.try_get("id")?;
            let revision: i64 = connection.try_get("revision")?;
            let candidate = self
                .search_resolution_connection(
                    actor,
                    &attempt.target,
                    installation,
                    revision,
                    &query,
                )
                .await;
            match candidate {
                Ok(Some(candidate)) => return Ok(Some(candidate)),
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => tracing::debug!(
                    event = "source_resolution_provider_failed",
                    error_kind = error.code(),
                    "Source resolution provider exchange failed"
                ),
                Ok(None) => {}
            }
        }
        Ok(None)
    }

    async fn search_resolution_connection(
        &self,
        actor: &Actor,
        target: &Target,
        installation: String,
        revision: i64,
        query: &str,
    ) -> Result<Option<Candidate>> {
        let operation = self
            .plugins
            .operation(
                actor,
                &installation,
                revision,
                &[Capability::Search, Capability::Sources],
            )
            .await?;
        let mut page = operation
            .search(&SearchRequest {
                query: query.into(),
                cursor: None,
                limit: 20,
            })
            .await?;
        {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            operation.recheck(&mut connection).await?;
        }
        page.items
            .retain(|candidate| target.musical.plausible(candidate));
        page.items.sort_by(|left, right| {
            target
                .musical
                .duration_distance(left)
                .cmp(&target.musical.duration_distance(right))
                .then_with(|| left.id.cmp(&right.id))
        });
        for summary in page.items.into_iter().take(3) {
            let request = RecordingRequest {
                recording_id: summary.id.clone(),
            };
            let result = operation.recording(&request).await;
            let (details, sources) = match result {
                Ok(result) => result,
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    tracing::debug!(
                        event = "source_resolution_candidate_failed",
                        error_kind = error.code(),
                        "Source resolution candidate exchange failed"
                    );
                    continue;
                }
            };
            if details.id != summary.id
                || !target.musical.accepts(&details)
                || !sources.items.iter().any(|source| source.ready)
            {
                continue;
            }
            return Ok(Some(Candidate {
                installation,
                installation_revision: revision,
                operation,
                details,
                sources,
            }));
        }
        Ok(None)
    }
}

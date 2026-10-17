use super::authority::{self, Target};
use crate::{Actor, Catalog, Error, Result, plugins::Operation};
use hito_plugin_sdk::{Capability, ProviderRecording, RecordingRequest, SearchRequest, SourcePage};
use std::{iter::Take, sync::Arc, vec::IntoIter};

pub(super) struct Candidate {
    pub installation: String,
    pub installation_revision: i64,
    pub operation: Arc<Operation>,
    pub details: ProviderRecording,
    pub sources: SourcePage,
}

pub(super) struct Page {
    installation: String,
    revision: i64,
    operation: Arc<Operation>,
    summaries: Take<IntoIter<ProviderRecording>>,
}

impl Catalog {
    pub(super) async fn search_resolution_connection(
        &self,
        actor: &Actor,
        target: &Target,
        installation: String,
        revision: i64,
        query: &str,
    ) -> Result<Page> {
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
        Ok(Page {
            installation,
            revision,
            operation: Arc::new(operation),
            summaries: page.items.into_iter().take(3),
        })
    }
}

impl Page {
    pub(super) async fn next(
        &mut self,
        catalog: &Catalog,
        actor: &Actor,
        recording: &str,
        target: &Target,
    ) -> Result<Option<Candidate>> {
        for summary in self.summaries.by_ref() {
            let occupied = {
                let mut connection = catalog.store.pool.acquire().await?;
                authority::access(&mut connection, actor, recording).await?;
                self.operation.recheck(&mut connection).await?;
                sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM provider_items candidate
                    JOIN provider_items primary_item ON primary_item.id = ?
                    WHERE candidate.installation_id = ? AND candidate.provider_recording_id = ?
                        AND candidate.retired = 0 AND candidate.recording_id != primary_item.recording_id)")
                    .bind(&target.binding_id).bind(&self.installation).bind(&summary.id)
                    .fetch_one(&mut *connection).await?
            };
            if occupied {
                tracing::debug!(
                    event = "source_resolution_candidate_skipped",
                    outcome = "already_bound",
                    "Source candidate belongs to another recording"
                );
                continue;
            }
            let request = RecordingRequest {
                recording_id: summary.id.clone(),
            };
            let result = self.operation.recording(&request).await;
            {
                let mut connection = catalog.store.pool.acquire().await?;
                authority::access(&mut connection, actor, recording).await?;
                self.operation.recheck(&mut connection).await?;
            }
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
                installation: self.installation.clone(),
                installation_revision: self.revision,
                operation: Arc::clone(&self.operation),
                details,
                sources,
            }));
        }
        Ok(None)
    }
}

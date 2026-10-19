mod authority;
mod transport;

pub(in crate::community) use transport::Transport;

use super::Community;
use crate::{Actor, Result, Source};
use authority::Snapshot;
use std::time::{Duration, Instant};
use uuid::Uuid;

const LOOKUP_BUDGET: Duration = Duration::from_millis(100);
const MAX_CANDIDATES: usize = 32;

pub(crate) enum SourceOrder {
    Unchanged,
    Preferred(Vec<String>),
    Reload,
}

impl Community {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "community_source_ranking", request_id = %Uuid::new_v4()))]
    pub(crate) async fn rank_sources(
        &self,
        actor: &Actor,
        recording: &str,
        sources: &[Source],
    ) -> Result<SourceOrder> {
        let started = Instant::now();
        let result = if let Some(remote) = &self.rankings {
            if sources.iter().any(|source| {
                source.ready && matches!(source.provider.as_str(), "local" | "download")
            }) {
                Ok(SourceOrder::Unchanged)
            } else {
                let mut changed = self.ranking_changed.subscribe();
                tokio::select! {
                    biased;
                    _ = changed.changed() => Ok(SourceOrder::Reload),
                    result = tokio::time::timeout(LOOKUP_BUDGET, self.read_ranking_order(remote, actor, recording, sources)) =>
                        result.unwrap_or(Ok(SourceOrder::Reload)),
                }
            }
        } else {
            Ok(SourceOrder::Unchanged)
        };
        tracing::debug!(
            outcome = match &result {
                Ok(SourceOrder::Unchanged) => "unchanged",
                Ok(SourceOrder::Preferred(_)) => "ranked",
                Ok(SourceOrder::Reload) => "fallback",
                Err(error) => error.code(),
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Community source ranking completed"
        );
        result
    }

    async fn read_ranking_order(
        &self,
        remote: &Transport,
        actor: &Actor,
        recording: &str,
        sources: &[Source],
    ) -> Result<SourceOrder> {
        let Some(before) =
            Snapshot::capture(&self.store, &self.host, actor, recording, sources).await?
        else {
            return Ok(SourceOrder::Unchanged);
        };
        let candidate_count = before.candidates.len();
        let scores = match remote.rank(actor.id(), &before.candidates).await {
            Ok(scores) => scores,
            Err(error) => {
                tracing::debug!(
                    outcome = error.code(),
                    candidate_count,
                    "Community ranking response discarded"
                );
                return Ok(SourceOrder::Reload);
            }
        };
        let after = Snapshot::capture(&self.store, &self.host, actor, recording, sources).await?;
        if after.as_ref() != Some(&before) {
            return Ok(SourceOrder::Reload);
        }
        tracing::debug!(
            outcome = "current",
            candidate_count,
            "Community ranking authority rechecked"
        );
        Ok(SourceOrder::Preferred(before.order(&scores)))
    }
}

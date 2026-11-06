use super::{Catalog, provider_installations, ranking_on};
use crate::{Actor, Error, Result, Source, community::rankings::SourceOrder};
use std::{
    collections::HashSet,
    future::Future,
    time::{Duration, Instant},
};
use uuid::Uuid;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "playback_sources", request_id = %Uuid::new_v4()))]
    pub async fn playback_sources_until(
        &self,
        actor: &Actor,
        recording: &str,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<Vec<Source>> {
        let started = Instant::now();
        tokio::pin!(cancelled);
        let read = async {
            let mut sources = self.read_playback_sources(actor, recording).await?;
            match self
                .community
                .rank_sources(actor, recording, &sources)
                .await?
            {
                SourceOrder::Unchanged => {}
                SourceOrder::Reload => {
                    sources = self.read_playback_sources(actor, recording).await?
                }
                SourceOrder::Preferred(order) => {
                    if !apply_order(&mut sources, &order) {
                        sources = self.read_playback_sources(actor, recording).await?;
                    }
                }
            }
            self.apply_preference(actor, recording, &mut sources)
                .await?;
            tokio::time::timeout(Duration::from_secs(1), actor.check_session(&self.store))
                .await
                .unwrap_or(Err(Error::Upstream(
                    "Playback authority could not be checked in time",
                )))?;
            Ok(sources)
        };
        let result: Result<Vec<Source>> = tokio::select! {
            biased;
            _ = cancelled.as_mut() => Err(Error::Upstream("Playback source selection was cancelled")),
            result = read => result,
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "ready"),
            source_count = result.as_ref().map_or(0, Vec::len),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Playback sources read completed"
        );
        result
    }

    async fn apply_preference(
        &self,
        actor: &Actor,
        recording: &str,
        sources: &mut [Source],
    ) -> Result<()> {
        let mut connection = self.store.pool.acquire().await?;
        let ranking = ranking_on(&mut connection, actor).await?;
        let installations = provider_installations(&mut connection, actor, recording).await?;
        ranking.sort_sources(sources, &installations);
        Ok(())
    }

    async fn read_playback_sources(&self, actor: &Actor, recording: &str) -> Result<Vec<Source>> {
        tokio::time::timeout(Duration::from_secs(1), async {
            actor.check_session(&self.store).await?;
            self.sources(actor, recording).await
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Playback sources could not be read in time",
        )))
    }
}

fn apply_order(sources: &mut [Source], order: &[String]) -> bool {
    let selected: HashSet<_> = order.iter().map(String::as_str).collect();
    let positions: Vec<_> = sources
        .iter()
        .enumerate()
        .filter_map(|(index, source)| selected.contains(source.id.as_str()).then_some(index))
        .collect();
    if positions.len() != order.len() || selected.len() != order.len() {
        return false;
    }
    for (target, source) in positions.iter().zip(order) {
        let Some(current) = positions
            .iter()
            .find(|index| sources[**index].id == *source)
        else {
            return false;
        };
        sources.swap(*target, *current);
    }
    true
}

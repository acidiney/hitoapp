mod ranking;
mod store;
mod types;

use super::Catalog;
use crate::{Actor, Error, Result};
pub(crate) use ranking::{Origin, SourceRanking};
use std::time::{Duration, Instant};
pub(crate) use store::{provider_installations, ranking_on};
pub use types::{SourcePreferenceEntry, SourcePreferences, UpdateSourcePreferences};
use uuid::Uuid;

const DEADLINE: Duration = Duration::from_secs(1);

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "source_preferences", request_id = %Uuid::new_v4()))]
    pub async fn source_preferences(&self, actor: &Actor) -> Result<SourcePreferences> {
        let started = Instant::now();
        let result = tokio::time::timeout(DEADLINE, async {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            let preferences = store::read_on(&mut tx, actor).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(preferences)
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Source preferences could not be read in time",
        )));
        tracing::debug!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "read"),
            entry_count = result.as_ref().map_or(0, |value| value.entries.len()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Source preferences read completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "source_preferences_update", request_id = %Uuid::new_v4()))]
    pub async fn update_source_preferences(
        &self,
        actor: &Actor,
        request: UpdateSourcePreferences,
    ) -> Result<SourcePreferences> {
        let started = Instant::now();
        let result = tokio::time::timeout(DEADLINE, async {
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            actor.check_session_on(&mut tx).await?;
            let saved = store::write_on(&mut tx, actor, request).await?;
            actor.check_session_on(&mut tx).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(saved)
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Source preferences could not be saved in time",
        )));
        tracing::info!(
            event = "source_preferences_update_finished",
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "saved"),
            entry_count = result.as_ref().map_or(0, |value| value.entries.len()),
            fallback = result.as_ref().ok().map(|value| value.fallback),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Source preferences update completed"
        );
        result
    }
}

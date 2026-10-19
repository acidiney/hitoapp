use super::{Community, CommunityRankingPreferences, UpdateCommunityRankingPreferences};
use crate::{Actor, Error, Result};
use std::time::{Duration, Instant};
use uuid::Uuid;

impl Community {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "community_ranking_preferences", request_id = %Uuid::new_v4()))]
    pub async fn ranking_preferences(&self, actor: &Actor) -> Result<CommunityRankingPreferences> {
        let started = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            let preferences = read(&mut tx, actor).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(preferences)
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community ranking preferences could not be read in time",
        )));
        tracing::debug!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "read"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Community ranking preferences read completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "community_ranking_preferences_update", request_id = %Uuid::new_v4()))]
    pub async fn update_ranking_preferences(
        &self,
        actor: &Actor,
        request: UpdateCommunityRankingPreferences,
    ) -> Result<CommunityRankingPreferences> {
        let started = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            actor.check_session_on(&mut tx).await?;
            let current = read(&mut tx, actor).await?;
            if current.revision != request.revision {
                return Err(Error::Conflict("Community ranking preferences changed. Review them again.".into()));
            }
            let revision = current.revision.checked_add(1).ok_or_else(|| {
                Error::Conflict("Community ranking preferences cannot be changed further".into())
            })?;
            sqlx::query("INSERT INTO account_community_ranking_preferences(user_id,revision,use_source_rankings)
                VALUES (?,?,?) ON CONFLICT(user_id) DO UPDATE SET revision=excluded.revision,
                use_source_rankings=excluded.use_source_rankings")
                .bind(actor.id()).bind(revision).bind(request.use_source_rankings)
                .execute(&mut *tx).await?;
            actor.check_session_on(&mut tx).await?;
            tx.commit().await?;
            self.ranking_changed.send_modify(|generation| *generation = generation.wrapping_add(1));
            actor.check_session(&self.store).await?;
            Ok(CommunityRankingPreferences { revision, use_source_rankings: request.use_source_rankings })
        })
        .await
        .unwrap_or(Err(Error::Upstream("Community ranking preferences could not be saved in time")));
        tracing::info!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "saved"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Community ranking preferences update completed"
        );
        result
    }
}

pub(super) async fn read(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
) -> Result<CommunityRankingPreferences> {
    Ok(sqlx::query_as("SELECT revision,use_source_rankings FROM account_community_ranking_preferences WHERE user_id=?")
        .bind(actor.id()).fetch_optional(connection).await?.unwrap_or_default())
}

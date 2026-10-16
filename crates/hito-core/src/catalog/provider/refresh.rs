use super::Catalog;
use crate::{Actor, Error, Result, Source, identity::recording_access, plugins::Operation};
use hito_plugin_sdk::{Capability, RecordingRequest, SourcePage};

#[derive(sqlx::FromRow)]
struct Binding {
    installation_id: String,
    provider_recording_id: String,
    revision: i64,
    observation_id: String,
    generation: i64,
}

struct Update {
    binding: Binding,
    operation: Operation,
    page: SourcePage,
}

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn refresh_sources(&self, actor: &Actor, recording: &str) -> Result<Vec<Source>> {
        self.refresh_sources_until(actor, recording, std::future::pending())
            .await?
            .ok_or(Error::Upstream("Source refresh was canceled"))
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn refresh_sources_until(
        &self,
        actor: &Actor,
        recording: &str,
        cancelled: impl std::future::Future<Output = ()> + Send,
    ) -> Result<Option<Vec<Source>>> {
        tokio::pin!(cancelled);
        let update = tokio::select! {
            biased;
            _ = &mut cancelled => {
                tracing::debug!(event = "provider_refresh_canceled", "Source refresh canceled before publication");
                return Ok(None);
            },
            result = self.prepare_refresh(actor, recording) => result?,
        };
        let Some(update) = update else {
            return tokio::select! {
                biased;
                _ = &mut cancelled => Ok(None),
                result = self.sources(actor, recording) => result.map(Some),
            };
        };
        tokio::select! {
            biased;
            _ = &mut cancelled => {
                tracing::debug!(event = "provider_refresh_canceled", "Source refresh canceled before publication");
                return Ok(None);
            },
            _ = std::future::ready(()) => {},
        }
        self.publish_refresh(actor, recording, update).await?;
        self.sources(actor, recording).await.map(Some)
    }

    async fn prepare_refresh(&self, actor: &Actor, recording: &str) -> Result<Option<Update>> {
        actor.check_session(&self.store).await?;
        let mut connection = self.store.pool.acquire().await?;
        recording_access(&mut connection, actor, recording).await?;
        let binding: Option<Binding> = sqlx::query_as(
            "SELECT item.installation_id,item.provider_recording_id,p.revision,state.observation_id,state.generation FROM provider_items item
             JOIN provider_source_state state ON state.recording_id = item.recording_id
             JOIN plugin_installations p ON p.id = item.installation_id AND p.active = 1
             WHERE item.recording_id = ? AND p.user_id = ?",
        ).bind(recording).bind(actor.id()).fetch_optional(&mut *connection).await?;
        drop(connection);
        let Some(binding) = binding else {
            return Ok(None);
        };
        let operation = self
            .plugins
            .operation(
                actor,
                &binding.installation_id,
                binding.revision,
                &[Capability::Sources],
            )
            .await?;
        let page = operation
            .sources(&RecordingRequest {
                recording_id: binding.provider_recording_id.clone(),
            })
            .await?;
        Ok(Some(Update {
            binding,
            operation,
            page,
        }))
    }

    async fn publish_refresh(&self, actor: &Actor, recording: &str, update: Update) -> Result<()> {
        tracing::debug!(
            event = "provider_refresh_write_started",
            "Publishing provider sources"
        );
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<()> = async {
            actor.check_session_on(&mut tx).await?;
            update.operation.recheck(&mut tx).await?;
            recording_access(&mut tx, actor, recording).await?;
            let ids = super::sources::replace(
                &mut tx,
                &update.binding.installation_id,
                recording,
                &update.page,
                super::sources::Snapshot {
                    generation: update.binding.generation,
                    observation: &update.binding.observation_id,
                },
            )
            .await?;
            for id in ids {
                super::sources::attach(&mut tx, &id, &update.binding.observation_id).await?;
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => tx.commit().await?,
            Err(error) => {
                tx.rollback().await?;
                return Err(error);
            }
        }
        tracing::info!(
            count = update.page.items.len(),
            "Provider playback sources refreshed"
        );
        Ok(())
    }
}

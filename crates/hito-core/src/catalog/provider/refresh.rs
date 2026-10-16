use super::Catalog;
use crate::{Actor, Error, Result, Source, identity::recording_access, plugins::Operation};
use hito_plugin_sdk::{Capability, RecordingRequest, SourcePage};

#[derive(sqlx::FromRow)]
struct Binding {
    binding_id: String,
    binding_revision: i64,
    installation_id: String,
    provider_recording_id: String,
    revision: i64,
    observation_id: String,
    generation: i64,
}

enum RefreshOutcome {
    Canceled,
    Unchanged,
    Published,
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
        match self
            .refresh_state_until(actor, recording, cancelled.as_mut())
            .await?
        {
            RefreshOutcome::Canceled => Ok(None),
            RefreshOutcome::Unchanged => tokio::select! {
                biased;
                _ = &mut cancelled => Ok(None),
                result = self.sources(actor, recording) => result.map(Some),
            },
            RefreshOutcome::Published => self.sources(actor, recording).await.map(Some),
        }
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn refresh_playback_sources_until(
        &self,
        actor: &Actor,
        recording: &str,
        cancelled: impl std::future::Future<Output = ()> + Send,
    ) -> Result<bool> {
        tokio::pin!(cancelled);
        Ok(!matches!(
            self.refresh_state_until(actor, recording, cancelled.as_mut())
                .await?,
            RefreshOutcome::Canceled
        ))
    }

    async fn refresh_state_until(
        &self,
        actor: &Actor,
        recording: &str,
        mut cancelled: std::pin::Pin<&mut (impl std::future::Future<Output = ()> + Send)>,
    ) -> Result<RefreshOutcome> {
        let bindings = tokio::select! {
            biased;
            _ = &mut cancelled => {
                tracing::debug!(event = "provider_refresh_canceled", "Source refresh canceled before publication");
                return Ok(RefreshOutcome::Canceled);
            },
            result = self.refresh_bindings(actor, recording) => result?,
        };
        if bindings.is_empty() {
            return Ok(RefreshOutcome::Unchanged);
        }
        let mut failure = None;
        for binding in bindings {
            let prepared = tokio::select! {
                biased;
                _ = &mut cancelled => return Ok(RefreshOutcome::Canceled),
                result = self.prepare_refresh(actor, binding) => result,
            };
            let update = match prepared {
                Ok(update) => update,
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    if failure.is_none() {
                        failure = Some(error);
                    }
                    continue;
                }
            };
            tokio::select! {
                biased;
                _ = &mut cancelled => return Ok(RefreshOutcome::Canceled),
                _ = std::future::ready(()) => {},
            }
            if let Err(error) = self.publish_refresh(actor, recording, update).await {
                if matches!(error, Error::Unauthorized) {
                    return Err(error);
                }
                if failure.is_none() {
                    failure = Some(error);
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(RefreshOutcome::Published)
    }

    async fn refresh_bindings(&self, actor: &Actor, recording: &str) -> Result<Vec<Binding>> {
        actor.check_session(&self.store).await?;
        let mut connection = self.store.pool.acquire().await?;
        recording_access(&mut connection, actor, recording).await?;
        Ok(sqlx::query_as(
            "SELECT item.id AS binding_id,item.revision AS binding_revision,item.installation_id,item.provider_recording_id,p.revision,state.observation_id,state.generation FROM provider_items item
             JOIN provider_source_state state ON state.binding_id = item.id
             JOIN plugin_installations p ON p.id = item.installation_id AND p.active = 1
             JOIN plugin_services service ON service.id = p.service_id AND service.active = 1
             WHERE item.recording_id = ? AND item.retired = 0 AND p.user_id = ?
             AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'sources')
             ORDER BY item.id LIMIT ?",
        ).bind(recording).bind(actor.id()).bind(super::bindings::MAX_PLAYBACK_BINDINGS as i64).fetch_all(&mut *connection).await?)
    }

    async fn prepare_refresh(&self, actor: &Actor, binding: Binding) -> Result<Update> {
        actor.check_session(&self.store).await?;
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
        Ok(Update {
            binding,
            operation,
            page,
        })
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
                &super::bindings::Binding {
                    id: update.binding.binding_id.clone(),
                    installation_id: update.binding.installation_id.clone(),
                    recording_id: recording.to_owned(),
                    revision: update.binding.binding_revision,
                },
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

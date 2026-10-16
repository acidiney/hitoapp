use super::Catalog;
use crate::{Actor, Result, Source, identity::recording_access};
use hito_plugin_sdk::{Capability, RecordingRequest};

#[derive(sqlx::FromRow)]
struct Binding {
    installation_id: String,
    provider_recording_id: String,
    revision: i64,
    observation_id: String,
    generation: i64,
}

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn refresh_sources(&self, actor: &Actor, recording: &str) -> Result<Vec<Source>> {
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
            return self.sources(actor, recording).await;
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
                recording_id: binding.provider_recording_id,
            })
            .await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        operation.recheck(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let ids = super::sources::replace(
            &mut tx,
            &binding.installation_id,
            recording,
            &page,
            super::sources::Snapshot {
                generation: binding.generation,
                observation: &binding.observation_id,
            },
        )
        .await?;
        for id in ids {
            super::sources::attach(&mut tx, &id, &binding.observation_id).await?;
        }
        tx.commit().await?;
        tracing::info!(
            count = page.items.len(),
            "Provider playback sources refreshed"
        );
        self.sources(actor, recording).await
    }
}

use super::{Catalog, metadata, persist};
use crate::{Actor, Error, Recording, Result};
use hito_plugin_sdk::{Capability, RecordingRequest};
use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdoptProviderRecording {
    pub installation_revision: i64,
    pub provider_recording_id: String,
}

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn adopt_provider(
        &self,
        actor: &Actor,
        installation: &str,
        request: AdoptProviderRecording,
    ) -> Result<Recording> {
        actor.check_session(&self.store).await?;
        let request_id = RecordingRequest {
            recording_id: request.provider_recording_id,
        };
        request_id
            .validate()
            .map_err(|message| Error::Invalid(message.into()))?;
        let operation = self
            .plugins
            .operation(
                actor,
                installation,
                request.installation_revision,
                &[Capability::Sources],
            )
            .await?;
        let (details, sources) = operation.recording(&request_id).await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        operation.recheck(&mut tx).await?;
        let library = crate::identity::personal_library(&mut tx, actor.id()).await?;
        let published = metadata::publish(
            &mut tx,
            &library,
            installation,
            &operation.provider,
            &details,
        )
        .await?;
        let recording = published.recording_id;
        let release = published.release_id;
        let observation = published.observation_id;
        persist::sources(
            &mut tx,
            &published.binding,
            release.as_deref(),
            &observation,
            &details,
            &sources,
        )
        .await?;
        crate::metadata::refresh_recording_credits(&mut tx, &recording).await?;
        if let Some(release) = &release {
            crate::metadata::refresh_release(&mut tx, actor.id(), release).await?;
        }
        let result = persist::selected(&mut tx, actor, &self.host, &recording).await?;
        tx.commit().await?;
        tracing::info!(sources = sources.items.len(), "Provider recording adopted");
        Ok(result)
    }
}

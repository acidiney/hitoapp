use crate::{Actor, Error, Result, identity::recording_access};
use hito_plugin_sdk::ProviderRecordingRef;
use sqlx::SqliteConnection;

#[derive(PartialEq, Eq, sqlx::FromRow)]
pub(super) struct Target {
    library_id: String,
    access_grant: String,
    binding_id: String,
    binding_revision: i64,
    observation_id: String,
    pub(super) installation_id: String,
    provider_recording_id: String,
    provider_revision: Option<String>,
}

impl Target {
    pub(super) async fn capture(
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
        observation: &str,
    ) -> Result<Self> {
        actor.check_session_on(connection).await?;
        recording_access(connection, actor, recording).await?;
        sqlx::query_as("SELECT recording.library_id,access.id AS access_grant,binding.id AS binding_id,
            binding.revision AS binding_revision,observation.id AS observation_id,
            binding.installation_id,observation.provider_recording_id,observation.provider_revision
            FROM recordings recording
            JOIN libraries library ON library.id=recording.library_id AND library.owner_id=?
            JOIN library_access_grants access ON access.library_id=library.id AND access.user_id=?
            JOIN metadata_observations observation ON observation.recording_id=recording.id AND observation.id=?
            JOIN provider_items binding ON binding.id=observation.provider_binding_id
                AND binding.recording_id=recording.id AND binding.retired=0
                AND binding.installation_id=observation.plugin_installation_id
                AND binding.provider_recording_id=observation.provider_recording_id
            JOIN plugin_installations installation ON installation.id=binding.installation_id
                AND installation.user_id=access.user_id AND installation.active=1
            JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
            LEFT JOIN provider_source_state source_state ON source_state.binding_id=binding.id
            WHERE recording.id=? AND observation.origin='provider_response' AND observation.source_id IS NULL
                AND ((binding.role='primary' AND recording.metadata_binding_id=binding.id
                    AND recording.metadata_observation_id=observation.id)
                    OR (binding.role='source' AND source_state.observation_id=observation.id))")
            .bind(actor.id()).bind(actor.id()).bind(observation).bind(recording)
            .fetch_optional(connection).await?.ok_or(Error::NotFound)
    }

    pub(super) fn reference(&self) -> ProviderRecordingRef {
        ProviderRecordingRef {
            id: self.provider_recording_id.clone(),
            revision: self.provider_revision.clone(),
        }
    }

    pub(super) async fn recheck(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
    ) -> Result<()> {
        let current = Self::capture(connection, actor, recording, &self.observation_id).await?;
        if current != *self {
            return Err(Error::Conflict(
                "The recording's provider identity changed. Reload it before continuing.".into(),
            ));
        }
        Ok(())
    }
}

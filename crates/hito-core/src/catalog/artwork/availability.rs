use super::Catalog;
use crate::{Actor, Result, identity::recording_access};
use serde::Serialize;
use sqlx::SqliteConnection;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct ArtworkAvailability {
    pub can_fetch: bool,
}

#[derive(sqlx::FromRow)]
pub(super) struct ApprovedProvider {
    pub observation_id: String,
    pub binding_id: String,
    pub binding_revision: i64,
    pub installation_id: String,
    pub installation_revision: i64,
    pub service_id: String,
    pub service_revision: i64,
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn artwork_availability(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<ArtworkAvailability> {
        let started = std::time::Instant::now();
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let can_fetch = approved_provider(&mut tx, actor.id(), recording)
            .await?
            .is_some();
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::debug!(
            can_fetch,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artwork availability read"
        );
        Ok(ArtworkAvailability { can_fetch })
    }
}

pub(super) async fn approved_provider(
    connection: &mut SqliteConnection,
    account: &str,
    recording: &str,
) -> Result<Option<ApprovedProvider>> {
    Ok(sqlx::query_as(
        "SELECT o.id AS observation_id,binding.id AS binding_id,binding.revision AS binding_revision,
            p.id AS installation_id,p.revision AS installation_revision,
            service.id AS service_id,service.revision AS service_revision
         FROM recordings r JOIN libraries l ON l.id = r.library_id AND l.owner_id = ?
         JOIN metadata_observations o ON o.id = r.metadata_observation_id AND o.recording_id = r.id
            AND o.origin = 'provider_response'
         JOIN provider_items binding ON binding.id = r.metadata_binding_id AND binding.id = o.provider_binding_id
            AND binding.recording_id = r.id AND binding.retired = 0
            AND binding.installation_id = o.plugin_installation_id AND binding.provider_recording_id = o.provider_recording_id
         JOIN plugin_installations p ON p.id = binding.installation_id AND p.user_id = ? AND p.active = 1
            AND json_extract(p.manifest,'$.id') = o.provider
         JOIN plugin_services service ON service.id = p.service_id AND service.active = 1
         WHERE r.id = ? AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'artwork')",
    ).bind(account).bind(account).bind(recording).fetch_optional(connection).await?)
}

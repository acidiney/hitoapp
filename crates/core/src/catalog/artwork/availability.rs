use super::Catalog;
use crate::{Actor, Result, identity::recording_access};
use serde::Serialize;
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct ArtworkAvailability {
    pub can_fetch: bool,
}

#[derive(sqlx::FromRow)]
pub(super) struct ApprovedProvider {
    pub recording_id: String,
    pub provider_release_id: Option<String>,
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
        actor.check_session(&self.store).await?;
        tracing::debug!(
            can_fetch,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artwork availability read"
        );
        Ok(ArtworkAvailability { can_fetch })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn release_artwork_availability(
        &self,
        actor: &Actor,
        release: &str,
    ) -> Result<ArtworkAvailability> {
        let started = std::time::Instant::now();
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        super::release::access_on(&mut tx, actor, release).await?;
        let can_fetch = automatic_artwork_allowed(&mut tx, actor.id()).await?
            && approved_release_provider(&mut tx, actor.id(), release)
                .await?
                .is_some();
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::debug!(
            can_fetch,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Album artwork availability read"
        );
        Ok(ArtworkAvailability { can_fetch })
    }
}

pub(super) async fn automatic_artwork_allowed(
    connection: &mut SqliteConnection,
    account: &str,
) -> Result<bool> {
    let allowed: Option<bool> =
        sqlx::query_scalar("SELECT artwork FROM account_metadata_automation WHERE user_id = ?")
            .bind(account)
            .fetch_optional(connection)
            .await?;
    Ok(allowed.unwrap_or(true))
}

pub(super) async fn approved_provider(
    connection: &mut SqliteConnection,
    account: &str,
    recording: &str,
) -> Result<Option<ApprovedProvider>> {
    let mut query = provider_query(account);
    query.push(" AND r.id = ").push_bind(recording.to_owned());
    Ok(query.build_query_as().fetch_optional(connection).await?)
}

pub(super) async fn approved_release_provider(
    connection: &mut SqliteConnection,
    account: &str,
    release: &str,
) -> Result<Option<ApprovedProvider>> {
    let mut query = provider_query(account);
    query.push(" AND r.release_id = ").push_bind(release.to_owned())
        .push(" AND o.release_id = r.release_id AND json_type(o.provider_metadata,'$.release.id') = 'text'
            ORDER BY r.metadata_observation_id,r.id LIMIT 1");
    Ok(query.build_query_as().fetch_optional(connection).await?)
}

fn provider_query(account: &str) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new(
        "SELECT r.id AS recording_id,json_extract(o.provider_metadata,'$.release.id') AS provider_release_id,
            o.id AS observation_id,binding.id AS binding_id,binding.revision AS binding_revision,
            p.id AS installation_id,p.revision AS installation_revision,
            service.id AS service_id,service.revision AS service_revision
         FROM recordings r CROSS JOIN libraries l ON l.id = r.library_id AND l.owner_id = ",
    );
    query.push_bind(account.to_owned()).push("
         CROSS JOIN metadata_observations o ON o.id = r.metadata_observation_id AND o.recording_id = r.id
            AND o.origin = 'provider_response'
         CROSS JOIN provider_items binding ON binding.id = r.metadata_binding_id AND binding.id = o.provider_binding_id
            AND binding.recording_id = r.id AND binding.retired = 0
            AND binding.installation_id = o.plugin_installation_id AND binding.provider_recording_id = o.provider_recording_id
         CROSS JOIN plugin_installations p ON p.id = binding.installation_id AND p.user_id = ")
        .push_bind(account.to_owned()).push(" AND p.active = 1
            AND json_extract(p.manifest,'$.id') = o.provider
         CROSS JOIN plugin_services service ON service.id = p.service_id AND service.active = 1
         WHERE EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'artwork')");
    query
}

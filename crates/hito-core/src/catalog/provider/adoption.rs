use super::{Catalog, metadata, persist};
use crate::{Actor, Error, Recording, Result};
use hito_plugin_sdk::{Capability, RecordingRequest};
use serde::Deserialize;
use sqlx::QueryBuilder;
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
        operation.recheck(&mut tx).await?;
        let library = persist::library(&mut tx, actor.id()).await?;
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
            installation,
            &recording,
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
        let mut query = QueryBuilder::new(
            "SELECT r.id,r.library_id,r.release_id,COALESCE(p.override_title,r.title) AS title,r.title AS inherited_title,COALESCE(p.override_artist,r.artist) AS artist,r.artist AS inherited_artist,a.title AS album,r.duration_ms,",
        );
        crate::catalog::push_available(&mut query, &self.host);
        query.push(" AS available,COALESCE(k.liked,0) AS liked FROM recordings r LEFT JOIN personal_recording_metadata p ON p.recording_id = r.id AND p.user_id = ").push_bind(actor.id().to_owned())
            .push(" LEFT JOIN account_releases a ON a.id = r.release_id AND a.user_id = ").push_bind(actor.id().to_owned())
            .push(" LEFT JOIN likes k ON k.recording_id = r.id AND k.user_id = ").push_bind(actor.id().to_owned())
            .push(" WHERE r.id = ").push_bind(recording);
        let result = query.build_query_as().fetch_one(&mut *tx).await?;
        tx.commit().await?;
        tracing::info!(sources = sources.items.len(), "Provider recording adopted");
        Ok(result)
    }
}

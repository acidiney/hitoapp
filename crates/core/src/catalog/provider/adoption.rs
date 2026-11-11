use super::{Catalog, bindings, metadata, persist, resolution::CatalogDecision};
use crate::{Actor, Error, Recording, Result};
use hito_plugin_sdk::{Capability, ProviderRecording, RecordingRequest};
use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdoptProviderRecording {
    pub installation_revision: i64,
    pub provider_recording_id: String,
}

pub(crate) struct AcceptedRecording {
    pub details: ProviderRecording,
    pub sources: Vec<(String, bool)>,
    pub decision: Option<CatalogDecision>,
}

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn adopt_provider(
        &self,
        actor: &Actor,
        installation: &str,
        request: AdoptProviderRecording,
    ) -> Result<Recording> {
        self.adopt_accepted(actor, installation, request, None)
            .await?
            .ok_or_else(|| Error::Internal("Provider adoption did not complete".into()))
    }

    pub(crate) async fn adopt_accepted(
        &self,
        actor: &Actor,
        installation: &str,
        request: AdoptProviderRecording,
        accepted: Option<&AcceptedRecording>,
    ) -> Result<Option<Recording>> {
        actor.check_session(&self.store).await?;
        let request_id = RecordingRequest {
            recording_id: request.provider_recording_id,
        };
        request_id
            .validate()
            .map_err(|message| Error::Invalid(message.into()))?;
        let operation = self
            .plugins
            .operation(actor, installation, request.installation_revision, &[])
            .await?;
        if !operation.offers(Capability::Sources)
            && !operation.offers(Capability::Search)
            && !operation.offers(Capability::Metadata)
        {
            return Err(Error::Invalid(
                "This plugin has not been approved for the requested capability".into(),
            ));
        }
        let (details, sources) = operation.recording_for_saving(&request_id).await?;
        if let Some(accepted) = accepted {
            let current: Vec<(String, bool)> = sources
                .iter()
                .flat_map(|page| &page.items)
                .map(|source| (source.id.clone(), source.ready))
                .collect();
            if details != accepted.details || current != accepted.sources {
                tracing::debug!(
                    event = "provider_adoption_refused",
                    outcome = "changed",
                    "Provider recording changed since it was accepted"
                );
                return Ok(None);
            }
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        operation.recheck(&mut tx).await?;
        if let Some(decision) = accepted.and_then(|accepted| accepted.decision.as_ref()) {
            decision.recheck(&mut tx).await?;
        }
        let library = crate::identity::personal_library(&mut tx, actor.id()).await?;
        if accepted.is_some_and(|accepted| accepted.decision.is_some())
            && bindings::current(&mut tx, installation, &details.id)
                .await?
                .is_some()
        {
            tracing::debug!(
                event = "provider_adoption_refused",
                outcome = "became_bound",
                "Provider recording became bound since it was accepted"
            );
            return Ok(None);
        }
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
        if let Some(sources) = &sources {
            persist::sources(
                &mut tx,
                &published.binding,
                release.as_deref(),
                &observation,
                &details,
                sources,
            )
            .await?;
        }
        if let Some(decision) = accepted.and_then(|accepted| accepted.decision.as_ref()) {
            decision
                .persist(&mut tx, actor, &recording, &published.binding.id)
                .await?;
        }
        crate::metadata::refresh_recording_credits(&mut tx, &recording).await?;
        if let Some(release) = &release {
            crate::metadata::refresh_release(&mut tx, actor.id(), release).await?;
        }
        let result = persist::selected(&mut tx, actor, &self.host, &recording).await?;
        tx.commit().await?;
        tracing::info!(
            sources = sources.as_ref().map(|page| page.items.len()),
            "Provider recording adopted"
        );
        Ok(Some(result))
    }
}

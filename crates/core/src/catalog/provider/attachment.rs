use super::{Catalog, bindings, evidence, persist};
use crate::{Actor, ChangeNotice, Error, Recording, Result, identity::recording_readers};
use hito_plugin_sdk::{Capability, ProviderRecording, RecordingRequest, SourcePage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::time::Instant;
use utoipa::ToSchema;

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderCandidate {
    pub installation_id: String,
    pub installation_revision: i64,
    pub provider_recording_id: String,
}

#[derive(Serialize, ToSchema)]
pub struct ProviderSourceReview {
    pub candidate: ProviderRecording,
    pub review_stamp: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachReviewedProviderSource {
    pub candidate: ProviderCandidate,
    pub review_stamp: String,
}

#[derive(PartialEq, Serialize, sqlx::FromRow)]
struct Target {
    library_id: String,
    metadata_binding_id: Option<String>,
    metadata_observation_id: Option<String>,
}

async fn target(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<Target> {
    sqlx::query_as("SELECT r.library_id,r.metadata_binding_id,r.metadata_observation_id FROM recordings r JOIN libraries l ON l.id = r.library_id AND l.owner_id = ? WHERE r.id = ?")
        .bind(actor.id()).bind(recording).fetch_optional(connection).await?.ok_or(Error::NotFound)
}

fn stamp(
    actor: &Actor,
    recording: &str,
    target: &Target,
    candidate: &ProviderCandidate,
    operation: &crate::plugins::Operation,
    details: &ProviderRecording,
) -> Result<String> {
    let review = serde_json::to_vec(&(
        "provider-source-v1",
        actor.id(),
        recording,
        target,
        candidate,
        &operation.provider,
        &operation.service,
        operation.service_revision,
        details,
    ))
    .map_err(|_| Error::Internal("Source review could not be encoded".into()))?;
    Ok(format!("provider-source-v1:{:x}", Sha256::digest(review)))
}

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn review_provider_source(
        &self,
        actor: &Actor,
        recording: &str,
        candidate: ProviderCandidate,
    ) -> Result<ProviderSourceReview> {
        let started = Instant::now();
        actor.check_session(&self.store).await?;
        let before = target(&mut *self.store.pool.acquire().await?, actor, recording).await?;
        let (operation, details, _) = self.attachment_candidate(actor, &candidate).await?;
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        operation.recheck(&mut tx).await?;
        if target(&mut tx, actor, recording).await? != before {
            return Err(Error::Conflict(
                "The recording changed during review. Review it again.".into(),
            ));
        }
        bindings::ensure_unbound(
            &mut tx,
            actor.id(),
            &candidate.installation_id,
            &candidate.provider_recording_id,
        )
        .await?;
        let review_stamp = stamp(actor, recording, &before, &candidate, &operation, &details)?;
        tx.commit().await?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Provider source reviewed"
        );
        Ok(ProviderSourceReview {
            candidate: details,
            review_stamp,
        })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn attach_provider_source(
        &self,
        actor: &Actor,
        recording: &str,
        request: AttachReviewedProviderSource,
    ) -> Result<Recording> {
        let started = Instant::now();
        actor.check_session(&self.store).await?;
        target(&mut *self.store.pool.acquire().await?, actor, recording).await?;
        let (operation, details, sources) =
            self.attachment_candidate(actor, &request.candidate).await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        operation.recheck(&mut tx).await?;
        let target = target(&mut tx, actor, recording).await?;
        let current_stamp = stamp(
            actor,
            recording,
            &target,
            &request.candidate,
            &operation,
            &details,
        )?;
        if request.review_stamp != current_stamp {
            return Err(Error::Conflict(
                "The recording or provider candidate changed. Review it again.".into(),
            ));
        }
        bindings::ensure_unbound(
            &mut tx,
            actor.id(),
            &request.candidate.installation_id,
            &details.id,
        )
        .await?;
        let binding = bindings::attach(
            &mut tx,
            &request.candidate.installation_id,
            &details.id,
            recording,
        )
        .await?;
        let release = persist::release(
            &mut tx,
            &target.library_id,
            &binding.installation_id,
            details.release.as_ref(),
            persist::ReleasePublication::PreserveExisting,
        )
        .await?;
        let observation = evidence::observe(
            &mut tx,
            &binding,
            &operation.provider,
            release.as_deref(),
            &details,
        )
        .await?;
        persist::sources(
            &mut tx,
            &binding,
            release.as_deref(),
            &observation,
            &details,
            &sources,
        )
        .await?;
        if let Some(release) = &release {
            crate::metadata::refresh_release(&mut tx, actor.id(), release).await?;
        }
        let result = persist::selected(&mut tx, actor, &self.host, recording).await?;
        let readers = recording_readers(&mut tx, recording).await?;
        tx.commit().await?;
        self.changes.publish_to(
            &readers,
            &[ChangeNotice::Sources {
                recording_id: recording.to_owned(),
            }],
        );
        tracing::info!(
            sources = sources.items.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            evidence = "user_confirmed",
            "Provider source attached"
        );
        Ok(result)
    }

    async fn attachment_candidate(
        &self,
        actor: &Actor,
        candidate: &ProviderCandidate,
    ) -> Result<(crate::plugins::Operation, ProviderRecording, SourcePage)> {
        let request = RecordingRequest {
            recording_id: candidate.provider_recording_id.clone(),
        };
        request
            .validate()
            .map_err(|message| Error::Invalid(message.into()))?;
        let operation = self
            .plugins
            .operation(
                actor,
                &candidate.installation_id,
                candidate.installation_revision,
                &[Capability::Sources],
            )
            .await?;
        let (details, sources) = operation.recording(&request).await?;
        Ok((operation, details, sources))
    }
}

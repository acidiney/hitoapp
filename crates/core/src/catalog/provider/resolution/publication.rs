use super::{
    SourceResolutionOutcome, authority,
    provider::Candidate,
    receipts::{self, Attempt},
};
use crate::catalog::provider::{
    bindings::{self, Binding},
    evidence, persist,
};
use crate::{Actor, Catalog, Error, Result};
use uuid::Uuid;

pub(super) enum Publication {
    Settled(SourceResolutionOutcome),
    CandidateOccupied,
}

impl Catalog {
    pub(super) async fn complete_resolution(
        &self,
        actor: &Actor,
        recording: &str,
        attempt: &Attempt,
        candidate: Option<Candidate>,
        provider_unavailable: bool,
    ) -> Result<Publication> {
        tracing::debug!(
            event = "source_resolution_write_started",
            resolution_id = attempt.id,
            account_id = actor.id(),
            "Publishing automatic source resolution"
        );
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        authority::access(&mut tx, actor, recording).await?;
        receipts::recheck_on(&mut tx, attempt).await?;
        let current = authority::capture(&mut tx, actor, recording).await?;
        let outcome = if authority::protected(&mut tx, actor, recording).await? {
            SourceResolutionOutcome::ProtectedChoice
        } else if current.digest()? != attempt.target_digest {
            SourceResolutionOutcome::Cancelled
        } else if let Some(candidate) = candidate {
            candidate.operation.recheck(&mut tx).await?;
            if !candidate.accepts(&current.musical) {
                return Err(Error::Conflict(
                    "The source candidate no longer matches this recording".into(),
                ));
            }
            let binding = match bindings::current(
                &mut tx,
                &candidate.installation,
                &candidate.details.id,
            )
            .await?
            {
                Some(binding) if binding.recording_id == recording => {
                    let automatic: bool = sqlx::query_scalar("SELECT evidence = 'automatic_policy' AND role = 'source' FROM provider_items WHERE id = ?")
                        .bind(&binding.id).fetch_one(&mut *tx).await?;
                    if !automatic {
                        return Err(Error::Conflict(
                            "This provider recording already has a protected binding".into(),
                        ));
                    }
                    binding
                }
                Some(_) => {
                    tx.rollback().await?;
                    tracing::debug!(
                        event = "source_resolution_candidate_skipped",
                        resolution_id = attempt.id,
                        outcome = "became_bound",
                        "Source candidate became associated with another recording"
                    );
                    return Ok(Publication::CandidateOccupied);
                }
                None => {
                    let binding = Binding {
                        id: Uuid::new_v4().to_string(),
                        installation_id: candidate.installation.clone(),
                        recording_id: recording.into(),
                        revision: 1,
                    };
                    sqlx::query("INSERT INTO provider_items(id,installation_id,provider_recording_id,recording_id,role,evidence) VALUES (?,?,?,?,'source','automatic_policy')")
                        .bind(&binding.id).bind(&binding.installation_id).bind(&candidate.details.id).bind(recording).execute(&mut *tx).await?;
                    binding
                }
            };
            let release = persist::release(
                &mut tx,
                &current.library_id,
                &candidate.installation,
                candidate.details.release.as_ref(),
                persist::ReleasePublication::PreserveExisting,
            )
            .await?;
            let observation = evidence::observe(
                &mut tx,
                &binding,
                &candidate.operation.provider,
                release.as_deref(),
                &candidate.details,
            )
            .await?;
            persist::sources(
                &mut tx,
                &binding,
                release.as_deref(),
                &observation,
                &candidate.details,
                &candidate.sources,
            )
            .await?;
            if let Some(release) = release.as_deref() {
                crate::metadata::refresh_release(&mut tx, actor.id(), release).await?;
            }
            let candidate_digest = candidate.digest()?;
            let musical_metadata = candidate.musical_metadata()?;
            sqlx::query("INSERT INTO automatic_source_decisions(resolution_id,binding_id,policy_version,target_digest,candidate_digest,installation_revision,service_revision,musical_metadata) VALUES (?,?,?,?,?,?,?,?)")
                .bind(&attempt.id).bind(&binding.id).bind(candidate.policy()).bind(&attempt.target_digest).bind(candidate_digest).bind(candidate.installation_revision).bind(candidate.operation.service_revision).bind(musical_metadata).execute(&mut *tx).await?;
            SourceResolutionOutcome::Ready
        } else if provider_unavailable {
            SourceResolutionOutcome::ProviderUnavailable
        } else {
            SourceResolutionOutcome::NoSuitableSource
        };
        receipts::finish_on(&mut tx, &attempt.id, Some(&attempt.token), outcome).await?;
        tx.commit().await?;
        tracing::info!(
            event = "source_resolution_committed",
            resolution_id = attempt.id,
            outcome = outcome.as_str(),
            "Source resolution committed"
        );
        Ok(Publication::Settled(outcome))
    }

    pub(super) async fn finish_available_resolution(
        &self,
        actor: &Actor,
        recording: &str,
        attempt: &Attempt,
        outcome: SourceResolutionOutcome,
    ) -> Result<SourceResolutionOutcome> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        authority::access(&mut tx, actor, recording).await?;
        receipts::recheck_on(&mut tx, attempt).await?;
        let current = authority::capture(&mut tx, actor, recording).await?;
        let outcome = if authority::protected(&mut tx, actor, recording).await? {
            SourceResolutionOutcome::ProtectedChoice
        } else if current.digest()? != attempt.target_digest {
            SourceResolutionOutcome::Cancelled
        } else {
            outcome
        };
        receipts::finish_on(&mut tx, &attempt.id, Some(&attempt.token), outcome).await?;
        tx.commit().await?;
        Ok(outcome)
    }
}

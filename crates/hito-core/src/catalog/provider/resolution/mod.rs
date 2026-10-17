mod authority;
mod matching;
mod provider;
mod publication;
mod receipts;
mod types;

use crate::{Actor, Catalog, Error, Result};
use receipts::{Admission, Attempt, Lease};
use std::{future::Future, time::Duration};
use tokio::time::Instant;
pub use types::{ResolveSource, SourceResolution, SourceResolutionOutcome};
use uuid::Uuid;

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn resolve_source_until(
        &self,
        actor: &Actor,
        recording: &str,
        request: ResolveSource,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<SourceResolution> {
        if request.request_id.is_nil() {
            return Err(Error::Invalid(
                "Use a nonempty source resolution request identity".into(),
            ));
        }
        let started = Instant::now();
        let deadline = started + Duration::from_secs(10);
        tokio::pin!(cancelled);
        let lease = Lease::new(&self.store);
        let admitted = tokio::select! {
            biased;
            _ = &mut cancelled => return Err(Error::Upstream("Source resolution was cancelled before admission")),
            _ = tokio::time::sleep_until(deadline) => return Err(resolution_timeout()),
            result = self.await_resolution_admission(actor, recording, request.request_id, lease.token()) => result?,
        };
        let (id, outcome) = match admitted {
            Admission::Execute(attempt) => {
                let outcome = tokio::select! {
                    biased;
                    _ = &mut cancelled => None,
                    _ = tokio::time::sleep_until(deadline) => None,
                    result = self.execute_resolution(actor, recording, &attempt) => Some(result),
                };
                match outcome {
                    Some(Ok(outcome)) => (attempt.id, outcome),
                    Some(Err(error)) => {
                        lease.cancel().await?;
                        return Err(error);
                    }
                    None => {
                        lease.cancel().await?;
                        (attempt.id, SourceResolutionOutcome::Cancelled)
                    }
                }
            }
            Admission::Observe {
                id,
                outcome: Some(outcome),
            } => (id, SourceResolutionOutcome::parse(&outcome)?),
            _ => {
                return Err(Error::Internal(
                    "Source resolution admission did not settle".into(),
                ));
            }
        };
        let mut connection = self.store.pool.acquire().await?;
        authority::access(&mut connection, actor, recording).await?;
        drop(connection);
        let sources = self.sources(actor, recording).await?;
        let resolution_id = Uuid::parse_str(&id)
            .map_err(|_| Error::Internal("Stored source resolution identity is invalid".into()))?;
        tracing::info!(event = "source_resolution_finished", resolution_id = %resolution_id, outcome = outcome.as_str(), elapsed_ms = started.elapsed().as_millis() as u64, "Source resolution finished");
        Ok(SourceResolution {
            request_id: request.request_id,
            resolution_id,
            recording_id: recording.into(),
            outcome,
            sources,
        })
    }

    async fn await_resolution_admission(
        &self,
        actor: &Actor,
        recording: &str,
        request: Uuid,
        token: &str,
    ) -> Result<Admission> {
        loop {
            match self
                .admit_resolution(actor, recording, request, token)
                .await?
            {
                Admission::Busy | Admission::Observe { outcome: None, .. } => {
                    tokio::time::sleep(Duration::from_millis(100)).await
                }
                settled => return Ok(settled),
            }
        }
    }

    async fn execute_resolution(
        &self,
        actor: &Actor,
        recording: &str,
        attempt: &Attempt,
    ) -> Result<SourceResolutionOutcome> {
        let sources = self.sources(actor, recording).await?;
        let protected = {
            let mut connection = self.store.pool.acquire().await?;
            authority::protected(&mut connection, recording).await?
        };
        if protected {
            return self
                .finish_available_resolution(
                    actor,
                    recording,
                    attempt,
                    SourceResolutionOutcome::ProtectedChoice,
                )
                .await;
        }
        if sources.iter().any(|source| source.ready) {
            return self
                .finish_available_resolution(
                    actor,
                    recording,
                    attempt,
                    SourceResolutionOutcome::Ready,
                )
                .await;
        }
        let candidate = self.resolution_candidates(actor, attempt).await?;
        self.complete_resolution(actor, recording, attempt, candidate)
            .await
    }
}

fn resolution_timeout() -> Error {
    Error::Upstream("Source resolution exceeded the time limit")
}

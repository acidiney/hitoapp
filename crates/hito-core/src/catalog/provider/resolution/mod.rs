mod admission;
mod authority;
mod cursor;
mod matching;
mod provider;
mod publication;
mod receipts;
mod types;

use crate::{Actor, Catalog, Error, Result};
use admission::Caller;
pub(crate) use admission::CallerAdmission;
use cursor::Candidates;
use publication::Publication;
use receipts::{Admission, Attempt, Lease};
use std::{future::Future, pin::Pin, time::Duration};
use tokio::{sync::oneshot, time::Instant};
pub use types::{ResolveSource, SourceResolution, SourceResolutionOutcome};
use uuid::Uuid;

enum PreparedResolution {
    Existing(SourceResolutionOutcome),
    Search(Box<Candidates>),
}

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
        let mut caller = self.resolution_callers.enter(actor.id())?;
        tokio::pin!(cancelled);
        tokio::select! {
            biased;
            _ = &mut cancelled => return Err(Error::Upstream("Source resolution was cancelled before admission")),
            _ = tokio::time::sleep_until(deadline) => return Err(resolution_timeout()),
            result = caller.wait() => result?,
        };
        let (cancel, cancellation) = oneshot::channel();
        let operation = self.resolve_source_owned(
            actor,
            recording,
            request,
            async move {
                let _ = cancellation.await;
            },
            &caller,
            started,
        );
        tokio::pin!(operation);
        tokio::select! {
            biased;
            _ = &mut cancelled => {
                caller.cancelling();
                let _ = cancel.send(());
                operation.await
            },
            result = &mut operation => result,
        }
    }

    async fn resolve_source_owned(
        &self,
        actor: &Actor,
        recording: &str,
        request: ResolveSource,
        cancelled: impl Future<Output = ()> + Send,
        caller: &Caller,
        started: Instant,
    ) -> Result<SourceResolution> {
        let deadline = started + Duration::from_secs(10);
        tokio::pin!(cancelled);
        let lease = Lease::new(&self.store);
        let admitted = tokio::select! {
            biased;
            _ = &mut cancelled => Err(Error::Upstream("Source resolution was cancelled before admission")),
            _ = tokio::time::sleep_until(deadline) => Err(resolution_timeout()),
            result = self.await_resolution_admission(actor, recording, request.request_id, lease.token()) => result,
        };
        let admitted = match admitted {
            Ok(admitted) => admitted,
            Err(error) => {
                caller.cancelling();
                lease.cancel().await?;
                return Err(error);
            }
        };
        let (id, outcome) = match admitted {
            Admission::Execute(attempt) => {
                let prepared = tokio::select! {
                    biased;
                    _ = &mut cancelled => None,
                    _ = tokio::time::sleep_until(deadline) => None,
                    result = self.prepare_resolution(actor, recording, &attempt) => Some(result),
                };
                let prepared = match prepared {
                    Some(Ok(prepared)) => tokio::select! {
                        biased;
                        _ = &mut cancelled => None,
                        _ = tokio::time::sleep_until(deadline) => None,
                        _ = std::future::ready(()) => Some(prepared),
                    },
                    Some(Err(error)) => {
                        caller.cancelling();
                        lease.cancel().await?;
                        return Err(error);
                    }
                    None => None,
                };
                let settled = match prepared {
                    Some(PreparedResolution::Existing(outcome)) => self
                        .finish_available_resolution(actor, recording, &attempt, outcome)
                        .await
                        .map(Some),
                    Some(PreparedResolution::Search(candidates)) => {
                        self.resolve_candidates(
                            actor,
                            recording,
                            &attempt,
                            *candidates,
                            cancelled.as_mut(),
                            deadline,
                        )
                        .await
                    }
                    None => Ok(None),
                };
                match settled {
                    Ok(Some(outcome)) => (attempt.id, outcome),
                    Ok(None) => {
                        caller.cancelling();
                        lease.cancel().await?;
                        (attempt.id, SourceResolutionOutcome::Cancelled)
                    }
                    Err(error) => {
                        caller.cancelling();
                        lease.cancel().await?;
                        return Err(error);
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

    async fn prepare_resolution(
        &self,
        actor: &Actor,
        recording: &str,
        attempt: &Attempt,
    ) -> Result<PreparedResolution> {
        let sources = self.sources(actor, recording).await?;
        let protected = {
            let mut connection = self.store.pool.acquire().await?;
            authority::protected(&mut connection, actor, recording).await?
        };
        if protected {
            return Ok(PreparedResolution::Existing(
                SourceResolutionOutcome::ProtectedChoice,
            ));
        }
        if sources.iter().any(|source| source.ready) {
            return Ok(PreparedResolution::Existing(SourceResolutionOutcome::Ready));
        }
        let candidates = Candidates::new(self, actor, &attempt.target).await?;
        Ok(PreparedResolution::Search(Box::new(candidates)))
    }

    async fn resolve_candidates(
        &self,
        actor: &Actor,
        recording: &str,
        attempt: &Attempt,
        mut candidates: Candidates,
        mut cancelled: Pin<&mut (impl Future<Output = ()> + Send)>,
        deadline: Instant,
    ) -> Result<Option<SourceResolutionOutcome>> {
        loop {
            let candidate = tokio::select! {
                biased;
                _ = cancelled.as_mut() => return Ok(None),
                _ = tokio::time::sleep_until(deadline) => return Ok(None),
                result = candidates.next(self, actor, recording, &attempt.target) => result?,
            };
            tokio::select! {
                biased;
                _ = cancelled.as_mut() => return Ok(None),
                _ = tokio::time::sleep_until(deadline) => return Ok(None),
                _ = std::future::ready(()) => {},
            }
            match self
                .complete_resolution(actor, recording, attempt, candidate)
                .await?
            {
                Publication::Settled(outcome) => return Ok(Some(outcome)),
                Publication::CandidateOccupied => {}
            }
        }
    }
}

fn resolution_timeout() -> Error {
    Error::Upstream("Source resolution exceeded the time limit")
}

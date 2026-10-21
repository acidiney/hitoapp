use super::{
    commands::{Action, CandidateSnapshot, Draft, Failure, Grant, Proof, Reply},
    connection::OpenState,
    records::{self, Phase, Record, Target},
};
use crate::{StateError, policy::Error};
use hito_debrid_torbox::{Error as ProviderError, GeneratedFileCandidate};
use hito_plugin_sdk::PreparationReason;
use sqlx::Connection;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

impl OpenState {
    pub(super) async fn execute(&mut self, action: Action) -> Result<Reply, Failure> {
        let target = match &action {
            Action::Submit { target, .. }
            | Action::Poll { target, .. }
            | Action::Invalidate { target, .. }
            | Action::Current(target)
            | Action::Snapshot(target) => target,
            Action::Intent { proof, .. } | Action::Finish { proof, .. } => &proof.target,
        };
        if !self.targets.contains(target) {
            return Err(Failure::Request(Error::NotFound));
        }
        match action {
            Action::Submit { operation, target } => self.submit(operation, target).await,
            Action::Poll {
                operation,
                handle,
                target,
            } => self.poll(operation, handle, target).await,
            Action::Intent { proof, lease } => self.intent(proof, lease).await,
            Action::Finish {
                proof,
                lease,
                result,
            } => self.finish(proof, lease, result).await,
            Action::Current(target) => {
                let record = self.current(&target).await?;
                Ok(Reply::Current(
                    record
                        .filter(|record| record.phase == Phase::Ready)
                        .map(|record| record.operation),
                ))
            }
            Action::Snapshot(target) => {
                let record = self
                    .current(&target)
                    .await?
                    .filter(|record| record.phase == Phase::Ready)
                    .ok_or(Failure::Request(Error::Unavailable))?;
                let candidate = self
                    .client
                    .restore_candidate(
                        &self.candidate_key,
                        record.context(self.state_id),
                        &record.candidate[..record.candidate_length],
                    )
                    .map_err(|_| StateError::InvalidState)?;
                Ok(Reply::Snapshot(CandidateSnapshot {
                    operation: record.operation,
                    value: candidate,
                }))
            }
            Action::Invalidate {
                target,
                operation,
                reason,
            } => self.invalidate(target, operation, reason).await,
        }
    }

    async fn submit(&mut self, operation: Uuid, target: Target) -> Result<Reply, Failure> {
        if let Some(record) = self.record(operation).await? {
            if record.target != target {
                return Err(Failure::Request(Error::Conflict));
            }
            let resume = record.phase == Phase::Checking && !self.leased(operation);
            return Ok(self.step(&record, resume));
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM operations")
            .fetch_one(&mut self.connection)
            .await
            .map_err(super::database_error)?;
        if count >= records::MAX_RECEIPTS as i64 {
            return Err(Failure::Request(Error::Busy));
        }
        let claims = records::claims(
            &mut self.connection,
            &self.key,
            self.state_id,
            &self.authority,
            &target,
        )
        .await?;
        if claims
            .iter()
            .any(|record| record.target != target || record.phase != Phase::Failed)
        {
            return Err(Failure::Request(Error::Conflict));
        }
        let previous = claims.into_iter().next();
        let record = Record::new(operation, target, self.authority);
        let started = Instant::now();
        self.persist_admission(&record, previous.as_ref()).await?;
        committed(&record, started);
        Ok(self.step(&record, true))
    }

    async fn poll(
        &mut self,
        operation: Uuid,
        handle: Uuid,
        target: Target,
    ) -> Result<Reply, Failure> {
        let record = self
            .record(operation)
            .await?
            .ok_or(Failure::Request(Error::NotFound))?;
        if record.target != target || record.handle != handle {
            return Err(Failure::Request(Error::NotFound));
        }
        if record.phase == Phase::Checking && !self.leased(operation) {
            return Ok(self.step(&record, true));
        }
        let due = matches!(record.phase, Phase::Preparing | Phase::Blocked)
            && records::now_ms()? >= record.retry_at_ms
            && self
                .retry_guards
                .get(&operation)
                .is_some_and(|deadline| Instant::now() >= *deadline);
        if due && record.current && !self.leased(operation) {
            let next = record.next(Phase::Checking, None)?;
            self.change(&record, &next).await?;
            return Ok(self.step(&next, true));
        }
        Ok(self.step(&record, false))
    }

    async fn intent(&mut self, proof: Proof, lease: Arc<()>) -> Result<Reply, Failure> {
        let record = self.owned(&proof, &lease).await?;
        if record.phase != Phase::Checking {
            return Err(Failure::Request(Error::Conflict));
        }
        let next = record.next(Phase::MintIntent, None)?;
        self.change(&record, &next).await?;
        Ok(Reply::Proof(proof_for(&next)))
    }

    async fn finish(
        &mut self,
        proof: Proof,
        lease: Arc<()>,
        result: Result<GeneratedFileCandidate, ProviderError>,
    ) -> Result<Reply, Failure> {
        let record = self.owned(&proof, &lease).await?;
        if !matches!(record.phase, Phase::Checking | Phase::MintIntent) {
            return Err(Failure::Request(Error::Conflict));
        }
        let next = match result {
            Ok(candidate) => {
                if record.phase != Phase::MintIntent {
                    return Err(StateError::InvalidState.into());
                }
                let sealed = self
                    .client
                    .seal_candidate(
                        &self.candidate_key,
                        record.context(self.state_id),
                        &candidate,
                    )
                    .map_err(|_| StateError::Unavailable)?;
                let mut next = record.next(Phase::Ready, None)?;
                if sealed.as_bytes().len() > next.candidate.len() {
                    return Err(StateError::InvalidState.into());
                }
                next.candidate[..sealed.as_bytes().len()].copy_from_slice(sealed.as_bytes());
                next.candidate_length = sealed.as_bytes().len();
                next
            }
            Err(error) => {
                let (phase, reason) = records::failed_phase(error);
                record.next(phase, reason)?
            }
        };
        self.change(&record, &next).await?;
        self.leases.remove(&record.operation);
        Ok(Reply::Status(next.status()))
    }

    async fn invalidate(
        &mut self,
        target: Target,
        operation: Uuid,
        reason: PreparationReason,
    ) -> Result<Reply, Failure> {
        if let Some(record) = self.current(&target).await?
            && record.operation == operation
            && (record.phase == Phase::Ready
                || (reason == PreparationReason::SourceChanged
                    && record.phase == Phase::Failed
                    && record.reason == Some(PreparationReason::SourceUnavailable)))
        {
            let next = record.next(Phase::Failed, Some(reason))?;
            self.change(&record, &next).await?;
        }
        Ok(Reply::Done)
    }

    async fn change(&mut self, old: &Record, next: &Record) -> Result<(), StateError> {
        let started = Instant::now();
        self.boundary().await?;
        let mut transaction = self
            .connection
            .begin()
            .await
            .map_err(super::database_error)?;
        self.header.verify(&mut transaction, &self.key).await?;
        records::update(&mut transaction, old, next, &self.key, self.state_id).await?;
        let header = self.header.refresh(&mut transaction, &self.key).await?;
        transaction.commit().await.map_err(super::database_error)?;
        self.boundary().await?;
        self.header = header;
        if matches!(next.phase, Phase::Preparing | Phase::Blocked) {
            self.retry_guards
                .insert(next.operation, Instant::now() + Duration::from_secs(5));
        } else {
            self.retry_guards.remove(&next.operation);
        }
        committed(next, started);
        Ok(())
    }

    async fn owned(&mut self, proof: &Proof, lease: &Arc<()>) -> Result<Record, Failure> {
        let record = self
            .record(proof.operation)
            .await?
            .ok_or(Failure::Request(Error::NotFound))?;
        let owned = self
            .leases
            .get(&proof.operation)
            .and_then(std::sync::Weak::upgrade)
            .is_some_and(|active| Arc::ptr_eq(&active, lease));
        if !owned
            || !record.current
            || record.target != proof.target
            || record.handle != proof.handle
            || record.revision != proof.revision
        {
            return Err(Failure::Request(Error::Conflict));
        }
        Ok(record)
    }

    async fn record(&mut self, operation: Uuid) -> Result<Option<Record>, StateError> {
        records::load(
            &mut self.connection,
            &self.key,
            self.state_id,
            &self.authority,
            operation,
        )
        .await
    }

    async fn current(&mut self, target: &Target) -> Result<Option<Record>, StateError> {
        let record = records::current(
            &mut self.connection,
            &self.key,
            self.state_id,
            &self.authority,
            target,
        )
        .await?;
        Ok(record.filter(|record| record.target == *target))
    }

    fn leased(&self, operation: Uuid) -> bool {
        self.leases
            .get(&operation)
            .and_then(std::sync::Weak::upgrade)
            .is_some()
    }

    fn step(&mut self, record: &Record, acquire: bool) -> Reply {
        let grant = if acquire {
            self.leases.retain(|_, lease| lease.strong_count() > 0);
            let lease = Arc::new(());
            self.leases.insert(record.operation, Arc::downgrade(&lease));
            Some(Grant {
                proof: proof_for(record),
                lease,
            })
        } else {
            None
        };
        Reply::Step(Draft {
            handle: record.handle.to_string(),
            status: record.status(),
            grant,
        })
    }
}

fn proof_for(record: &Record) -> Proof {
    Proof {
        operation: record.operation,
        handle: record.handle,
        target: record.target.clone(),
        revision: record.revision,
    }
}

fn committed(record: &Record, started: Instant) {
    let phase = match record.phase {
        Phase::Checking => "checking",
        Phase::MintIntent => "mint_intent",
        Phase::Preparing => "preparing",
        Phase::Blocked => "blocked",
        Phase::Ready => "ready",
        Phase::Unknown => "unknown",
        Phase::Failed => "failed",
    };
    tracing::debug!(
        event = "torbox_journal_transition_committed",
        outcome = phase,
        count = 1_u64,
        elapsed_ms = started.elapsed().as_millis() as u64
    );
}

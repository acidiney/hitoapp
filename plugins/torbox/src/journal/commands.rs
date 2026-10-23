use super::{Log, connection::OpenState, records::Target};
use crate::{StateError, policy::Error};
use hito_debrid_torbox::{Error as ProviderError, GeneratedFileCandidate};
use hito_plugin_sdk::{PreparationReason, PreparationStatus};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{mpsc, oneshot};
use tracing::{Instrument, instrument::WithSubscriber};
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct Handle {
    pub(super) gate: Arc<AtomicBool>,
    pub(super) commands: mpsc::Sender<Envelope>,
}

pub(crate) struct Step {
    pub(crate) handle: String,
    pub(crate) status: PreparationStatus,
    pub(crate) attempt: Option<Attempt>,
}

pub(crate) struct CandidateSnapshot {
    pub(crate) operation: Uuid,
    pub(crate) value: GeneratedFileCandidate,
}

pub(crate) struct Attempt {
    handle: Handle,
    proof: Proof,
    lease: Arc<()>,
}

#[derive(Clone)]
pub(super) struct Proof {
    pub(super) operation: Uuid,
    pub(super) handle: Uuid,
    pub(super) revision: i64,
    pub(super) target: Target,
}

pub(super) struct Grant {
    pub(super) proof: Proof,
    pub(super) lease: Arc<()>,
}

pub(super) struct Draft {
    pub(super) handle: String,
    pub(super) status: PreparationStatus,
    pub(super) grant: Option<Grant>,
}

pub(super) enum Action {
    Submit {
        operation: Uuid,
        target: Target,
    },
    Poll {
        operation: Uuid,
        handle: Uuid,
        target: Target,
    },
    Current(Target),
    Snapshot(Target),
    Intent {
        proof: Proof,
        lease: Arc<()>,
    },
    Finish {
        proof: Proof,
        lease: Arc<()>,
        result: Result<GeneratedFileCandidate, ProviderError>,
    },
    Invalidate {
        target: Target,
        operation: Uuid,
        reason: PreparationReason,
    },
}

pub(super) enum Reply {
    Step(Draft),
    Current(Option<Uuid>),
    Snapshot(CandidateSnapshot),
    Proof(Proof),
    Status(PreparationStatus),
    Done,
}

pub(super) enum Failure {
    Request(Error),
    State(StateError),
}

impl From<StateError> for Failure {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

pub(super) struct Envelope {
    action: Action,
    reply: oneshot::Sender<Result<Reply, Error>>,
    log: Log,
}

impl Handle {
    pub(crate) async fn submit(&self, operation: Uuid, target: Target) -> Result<Step, Error> {
        self.step(Action::Submit { operation, target }).await
    }

    pub(crate) async fn poll(
        &self,
        operation: Uuid,
        target: Target,
        handle: &str,
    ) -> Result<Step, Error> {
        let parsed = Uuid::parse_str(handle).map_err(|_| Error::NotFound)?;
        if parsed.to_string() != handle {
            return Err(Error::NotFound);
        }
        self.step(Action::Poll {
            operation,
            handle: parsed,
            target,
        })
        .await
    }

    pub(crate) async fn candidate(&self, target: Target) -> Result<Option<Uuid>, Error> {
        match self.request(Action::Current(target)).await? {
            Reply::Current(operation) => Ok(operation),
            _ => Err(Error::Unavailable),
        }
    }

    pub(crate) async fn snapshot(&self, target: Target) -> Result<CandidateSnapshot, Error> {
        match self.request(Action::Snapshot(target)).await? {
            Reply::Snapshot(snapshot) => Ok(snapshot),
            _ => Err(Error::Unavailable),
        }
    }

    pub(crate) async fn invalidate(
        &self,
        target: Target,
        operation: Uuid,
        reason: PreparationReason,
    ) -> Result<(), Error> {
        match self
            .request(Action::Invalidate {
                target,
                operation,
                reason,
            })
            .await?
        {
            Reply::Done => Ok(()),
            _ => Err(Error::Unavailable),
        }
    }

    async fn step(&self, action: Action) -> Result<Step, Error> {
        match self.request(action).await? {
            Reply::Step(draft) => Ok(Step {
                handle: draft.handle,
                status: draft.status,
                attempt: draft.grant.map(|grant| Attempt {
                    handle: self.clone(),
                    proof: grant.proof,
                    lease: grant.lease,
                }),
            }),
            _ => Err(Error::Unavailable),
        }
    }

    async fn request(&self, action: Action) -> Result<Reply, Error> {
        let mut log = Log::new(action.operation(), "torbox_journal_command_finished");
        if !self.gate.load(Ordering::Acquire) {
            log.outcome = "closed";
            return Err(Error::Unavailable);
        }
        let (reply, response) = oneshot::channel();
        if let Err(error) = self.commands.try_send(Envelope { action, reply, log }) {
            return match error {
                mpsc::error::TrySendError::Full(mut envelope) => {
                    envelope.log.outcome = "busy";
                    Err(Error::Busy)
                }
                mpsc::error::TrySendError::Closed(mut envelope) => {
                    envelope.log.outcome = "closed";
                    Err(Error::Unavailable)
                }
            };
        }
        let result = response.await.map_err(|_| Error::Unavailable)?;
        if !self.gate.load(Ordering::Acquire) {
            return Err(Error::Unavailable);
        }
        result
    }
}

impl Attempt {
    pub(crate) async fn mint_intent(&mut self) -> Result<(), Error> {
        let reply = self
            .handle
            .request(Action::Intent {
                proof: self.proof.clone(),
                lease: self.lease.clone(),
            })
            .await?;
        match reply {
            Reply::Proof(proof) => {
                self.proof = proof;
                Ok(())
            }
            _ => Err(Error::Unavailable),
        }
    }

    pub(crate) async fn finish(
        self,
        result: Result<GeneratedFileCandidate, ProviderError>,
    ) -> Result<PreparationStatus, Error> {
        match self
            .handle
            .request(Action::Finish {
                proof: self.proof,
                lease: self.lease,
                result,
            })
            .await?
        {
            Reply::Status(status) => Ok(status),
            _ => Err(Error::Unavailable),
        }
    }
}

impl Action {
    fn operation(&self) -> &'static str {
        match self {
            Self::Submit { .. } => "receipt_admission",
            Self::Poll { .. } => "receipt_status",
            Self::Current(_) => "candidate_current",
            Self::Snapshot(_) => "candidate_snapshot",
            Self::Intent { .. } => "mint_intent",
            Self::Finish { .. } => "receipt_completion",
            Self::Invalidate { .. } => "candidate_invalidation",
        }
    }
}

pub(super) async fn serve(
    state: &mut OpenState,
    mut commands: mpsc::Receiver<Envelope>,
    mut stopped: oneshot::Receiver<()>,
    gate: &Arc<AtomicBool>,
) -> Result<(), StateError> {
    let mut fault = None;
    loop {
        tokio::select! {
            biased;
            _ = &mut stopped => break,
            command = commands.recv() => match command {
                Some(command) => dispatch(state, command, gate, &mut fault).await,
                None => break,
            },
        }
    }
    gate.store(false, Ordering::Release);
    commands.close();
    while let Some(command) = commands.recv().await {
        dispatch(state, command, gate, &mut fault).await;
    }
    fault.map_or(Ok(()), Err)
}

async fn dispatch(
    state: &mut OpenState,
    envelope: Envelope,
    gate: &Arc<AtomicBool>,
    fault: &mut Option<StateError>,
) {
    let Envelope {
        action,
        reply,
        mut log,
    } = envelope;
    if fault.is_some() {
        log.outcome = "poisoned";
        let _ = reply.send(Err(Error::Unavailable));
        return;
    }
    let result = state
        .execute(action)
        .instrument(log.span.clone())
        .with_subscriber(log.dispatch.clone())
        .await;
    let result = match result {
        Ok(reply) => {
            log.outcome = "complete";
            log.count = 1;
            Ok(reply)
        }
        Err(Failure::Request(error)) => {
            log.outcome = "rejected";
            Err(error)
        }
        Err(Failure::State(error)) => {
            *fault = Some(error);
            gate.store(false, Ordering::Release);
            log.outcome = "poisoned";
            Err(Error::Unavailable)
        }
    };
    if reply.send(result).is_err() && fault.is_none() {
        log.outcome = "waiter_gone";
    }
}

use crate::policy::Error;
use hito_debrid_torbox::{Error as ProviderError, GeneratedFileCandidate};
use hito_plugin_sdk::{PreparationReason, PreparationStatus};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(crate) struct Operations {
    inner: Mutex<Ledger>,
}
struct Ledger {
    records: HashMap<Uuid, Record>,
    current: Vec<Option<Uuid>>,
}
struct Record {
    index: usize,
    handle: Uuid,
    phase: Phase,
}
enum Phase {
    Running,
    Preparing(Instant),
    Blocked(PreparationReason, Instant),
    Ready(GeneratedFileCandidate),
    Unknown,
    Consumed,
    Failed(PreparationReason),
}

pub(crate) struct Step {
    pub(crate) handle: String,
    pub(crate) status: PreparationStatus,
    pub(crate) attempt: Option<Attempt>,
}

pub(crate) struct Attempt {
    owner: Arc<Operations>,
    operation: Uuid,
    armed: bool,
}

impl Operations {
    pub(crate) fn new(entries: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Ledger {
                records: HashMap::new(),
                current: vec![None; entries],
            }),
        })
    }

    pub(crate) fn submit(self: &Arc<Self>, operation: Uuid, index: usize) -> Result<Step, Error> {
        let mut ledger = self.inner.lock().map_err(|_| Error::Unavailable)?;
        if let Some(record) = ledger.records.get(&operation) {
            if record.index != index {
                return Err(Error::Conflict);
            }
            return Ok(record.step(None));
        }
        if ledger.records.len() >= 128 {
            return Err(Error::Busy);
        }
        if let Some(id) = ledger.current[index] {
            let record = ledger.records.get(&id).ok_or(Error::Unavailable)?;
            if !matches!(record.phase, Phase::Consumed | Phase::Failed(_)) {
                return Err(Error::Conflict);
            }
        }
        let record = Record {
            index,
            handle: Uuid::new_v4(),
            phase: Phase::Running,
        };
        let step = record.step(Some(Attempt {
            owner: self.clone(),
            operation,
            armed: true,
        }));
        ledger.records.insert(operation, record);
        ledger.current[index] = Some(operation);
        Ok(step)
    }

    pub(crate) fn poll(
        self: &Arc<Self>,
        operation: Uuid,
        index: usize,
        handle: &str,
    ) -> Result<Step, Error> {
        let mut ledger = self.inner.lock().map_err(|_| Error::Unavailable)?;
        let record = ledger.records.get_mut(&operation).ok_or(Error::NotFound)?;
        if record.index != index || record.handle.to_string() != handle {
            return Err(Error::NotFound);
        }
        let due = match record.phase {
            Phase::Preparing(at) | Phase::Blocked(_, at) => Instant::now() >= at,
            _ => false,
        };
        let attempt = if due {
            record.phase = Phase::Running;
            Some(Attempt {
                owner: self.clone(),
                operation,
                armed: true,
            })
        } else {
            None
        };
        Ok(record.step(attempt))
    }

    pub(crate) fn candidate(&self, index: usize) -> Result<Option<Uuid>, Error> {
        let ledger = self.inner.lock().map_err(|_| Error::Unavailable)?;
        Ok(ledger.current[index].filter(|id| {
            ledger
                .records
                .get(id)
                .is_some_and(|r| matches!(r.phase, Phase::Ready(_)))
        }))
    }

    pub(crate) fn invalidate(
        &self,
        index: usize,
        operation: Uuid,
        reason: PreparationReason,
    ) -> Result<(), Error> {
        let mut ledger = self.inner.lock().map_err(|_| Error::Unavailable)?;
        if ledger.current[index] == Some(operation) {
            let record = ledger
                .records
                .get_mut(&operation)
                .ok_or(Error::Unavailable)?;
            if matches!(record.phase, Phase::Ready(_)) {
                record.phase = Phase::Failed(reason);
            }
        }
        Ok(())
    }

    pub(crate) fn take(&self, index: usize) -> Result<GeneratedFileCandidate, Error> {
        let mut ledger = self.inner.lock().map_err(|_| Error::Unavailable)?;
        let id = ledger.current[index].ok_or(Error::Unavailable)?;
        let record = ledger.records.get_mut(&id).ok_or(Error::Unavailable)?;
        if !matches!(record.phase, Phase::Ready(_)) {
            return Err(Error::Unavailable);
        }
        match std::mem::replace(&mut record.phase, Phase::Consumed) {
            Phase::Ready(candidate) => Ok(candidate),
            _ => Err(Error::Unavailable),
        }
    }
}

impl Record {
    fn step(&self, attempt: Option<Attempt>) -> Step {
        Step {
            handle: self.handle.to_string(),
            status: self.phase.status(),
            attempt,
        }
    }
}

impl Phase {
    fn status(&self) -> PreparationStatus {
        match self {
            Self::Running | Self::Unknown => PreparationStatus::SubmissionUnknown {},
            Self::Preparing(_) => PreparationStatus::Preparing {
                progress_basis_points: None,
                eta_seconds: None,
                poll_after_seconds: 5,
            },
            Self::Blocked(reason, _) => PreparationStatus::Blocked {
                reason: *reason,
                retry_after_seconds: Some(5),
            },
            Self::Ready(_) => PreparationStatus::Ready {},
            Self::Consumed => PreparationStatus::Failed {
                reason: PreparationReason::SourceUnavailable,
            },
            Self::Failed(reason) => PreparationStatus::Failed { reason: *reason },
        }
    }

    fn from_result(result: Result<GeneratedFileCandidate, ProviderError>) -> Self {
        let retry = Instant::now() + Duration::from_secs(5);
        match result {
            Ok(candidate) => Self::Ready(candidate),
            Err(ProviderError::SourceIncomplete) => Self::Preparing(retry),
            Err(ProviderError::LinkUnknown) => Self::Unknown,
            Err(ProviderError::Status(401 | 403) | ProviderError::InvalidCredential) => {
                Self::Failed(PreparationReason::AuthorizationRequired)
            }
            Err(ProviderError::SourceChanged) => Self::Failed(PreparationReason::SourceChanged),
            Err(ProviderError::ProviderRejected) => {
                Self::Failed(PreparationReason::SourceUnavailable)
            }
            Err(ProviderError::ResponseTooLarge | ProviderError::InvalidResponse) => {
                Self::Failed(PreparationReason::InvalidResponse)
            }
            Err(ProviderError::Busy | ProviderError::Status(429)) => {
                Self::Blocked(PreparationReason::RateLimited, retry)
            }
            Err(_) => Self::Blocked(PreparationReason::ProviderUnavailable, retry),
        }
    }
}

impl Attempt {
    pub(crate) fn finish(
        mut self,
        result: Result<GeneratedFileCandidate, ProviderError>,
    ) -> Result<PreparationStatus, Error> {
        let phase = Phase::from_result(result);
        let status = phase.status();
        let mut ledger = self.owner.inner.lock().map_err(|_| Error::Unavailable)?;
        let record = ledger
            .records
            .get_mut(&self.operation)
            .ok_or(Error::Unavailable)?;
        record.phase = phase;
        self.armed = false;
        Ok(status)
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        if self.armed
            && let Ok(mut ledger) = self.owner.inner.lock()
            && let Some(record) = ledger.records.get_mut(&self.operation)
            && matches!(record.phase, Phase::Running)
        {
            record.phase = Phase::Unknown;
        }
    }
}

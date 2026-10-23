use crate::{
    Entry,
    http::{Error, StateData},
    receipts::Ticket,
};
use hito_debrid_all_debrid::{
    DelayedFileReceipt, DelayedPoll, GeneratedFileCandidate, MagnetPhase, PreparedFile,
};
use hito_plugin_sdk::{
    PreparationReason, PreparationRequest, PreparationStatus, PreparationStatusRequest,
    PreparationStatusResponse, PreparationSubmission, PreparationSubmitResponse,
};
use std::{
    sync::{Arc, MutexGuard},
    time::Duration,
};
use tokio::time::Instant;
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct Slot {
    operation: Option<Uuid>,
    handle: String,
    stage: Stage,
}
#[derive(Default)]
enum Stage {
    #[default]
    Empty,
    Mutating,
    Pending(DelayedFileReceipt),
    Polling,
    Candidate {
        value: GeneratedFileCandidate,
        revision: Uuid,
        receipt: Option<DelayedFileReceipt>,
    },
    Opening,
    Retryable {
        reason: PreparationReason,
        not_before: Instant,
    },
    Unavailable,
    Unknown,
}

enum Fallback {
    Unknown,
    Pending,
    Unavailable,
}
struct Lease {
    entry: Arc<Entry>,
    operation: Uuid,
    receipt: Option<DelayedFileReceipt>,
    fallback: Fallback,
    finished: bool,
}
impl Lease {
    fn complete(&mut self, stage: Stage, handle: Option<String>) -> Result<(), Error> {
        let mut slot = self.entry.slot.lock().map_err(|_| Error::Busy)?;
        if slot.operation != Some(self.operation) {
            return Err(Error::NotFound);
        }
        slot.stage = stage;
        if let Some(handle) = handle {
            slot.handle = handle;
        }
        self.finished = true;
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Ok(mut slot) = self.entry.slot.lock()
            && slot.operation == Some(self.operation)
        {
            slot.stage = match self.fallback {
                Fallback::Unknown => Stage::Unknown,
                Fallback::Pending => self
                    .receipt
                    .take()
                    .map(Stage::Pending)
                    .unwrap_or(Stage::Unavailable),
                Fallback::Unavailable => Stage::Unavailable,
            };
        }
    }
}

pub(crate) fn has_candidate(entry: &Entry) -> Result<bool, Error> {
    let slot = entry.slot.lock().map_err(|_| Error::Busy)?;
    Ok(matches!(slot.stage, Stage::Candidate { .. }))
}
pub(crate) fn invalidate(entry: &Entry) -> Result<(), Error> {
    let mut slot = entry.slot.lock().map_err(|_| Error::Busy)?;
    if matches!(slot.stage, Stage::Candidate { .. }) {
        slot.stage = Stage::Unavailable;
    }
    Ok(())
}

fn preparing(eta: Option<u32>) -> PreparationStatus {
    PreparationStatus::Preparing {
        progress_basis_points: None,
        eta_seconds: eta,
        poll_after_seconds: 5,
    }
}
fn summary(slot: &Slot) -> PreparationStatus {
    match slot.stage {
        Stage::Unavailable | Stage::Empty => PreparationStatus::Failed {
            reason: PreparationReason::SourceUnavailable,
        },
        Stage::Unknown => PreparationStatus::SubmissionUnknown {},
        _ => preparing(None),
    }
}

async fn current_status(
    state: &StateData,
    entry: &Entry,
    operation: Uuid,
    handle: &str,
) -> Result<PreparationStatus, Error> {
    let revision = {
        let slot = entry.slot.lock().map_err(|_| Error::Busy)?;
        if slot.operation != Some(operation) || slot.handle != handle {
            return Ok(PreparationStatus::Failed {
                reason: PreparationReason::SourceChanged,
            });
        }
        match slot.stage {
            Stage::Candidate { revision, .. } => revision,
            _ => return Ok(summary(&slot)),
        }
    };
    let observed = state.client.observe(&entry.configured.binding).await;
    let mut slot = entry.slot.lock().map_err(|_| Error::Busy)?;
    if slot.operation != Some(operation) || slot.handle != handle {
        return Ok(PreparationStatus::Failed {
            reason: PreparationReason::SourceChanged,
        });
    }
    if !matches!(slot.stage, Stage::Candidate { revision: current, .. } if current == revision) {
        return Ok(summary(&slot));
    }
    Ok(match observed {
        Ok(observed) if observed.phase == MagnetPhase::ProviderCompleted => {
            PreparationStatus::Ready {}
        }
        Ok(_) => preparing(None),
        Err(
            hito_debrid_all_debrid::Error::SourceChanged
            | hito_debrid_all_debrid::Error::ProviderFailed,
        ) => {
            slot.stage = Stage::Unavailable;
            PreparationStatus::Failed {
                reason: PreparationReason::SourceUnavailable,
            }
        }
        Err(error) => PreparationStatus::Blocked {
            reason: reason(error),
            retry_after_seconds: Some(5),
        },
    })
}

fn submission(slot: &Slot) -> PreparationSubmission {
    if let Stage::Retryable { reason, .. } = slot.stage {
        PreparationSubmission::NotSubmitted {
            reason,
            retry_after_seconds: Some(5),
        }
    } else if matches!(slot.stage, Stage::Unknown | Stage::Mutating) {
        PreparationSubmission::SubmissionUnknown {}
    } else if slot.handle.is_empty() {
        PreparationSubmission::NotSubmitted {
            reason: PreparationReason::SourceUnavailable,
            retry_after_seconds: None,
        }
    } else {
        PreparationSubmission::Accepted {
            handle: slot.handle.clone(),
            status: summary(slot),
        }
    }
}

pub(crate) async fn submit(
    state: &StateData,
    entry: Arc<Entry>,
    request: &PreparationRequest,
) -> Result<PreparationSubmitResponse, Error> {
    let replay = {
        let _claim = admit_operation(state, &entry, request.operation_id)?;
        let mut slot = entry.slot.lock().map_err(|_| Error::Busy)?;
        let retry_admitted = matches!(slot.stage, Stage::Retryable { not_before, .. }
            if Instant::now() >= not_before);
        if slot.operation == Some(request.operation_id) && !retry_admitted {
            Some(submission(&slot))
        } else if slot.operation.is_some()
            && !matches!(slot.stage, Stage::Unavailable)
            && !retry_admitted
        {
            Some(PreparationSubmission::NotSubmitted {
                reason: PreparationReason::RateLimited,
                retry_after_seconds: Some(5),
            })
        } else {
            slot.operation = Some(request.operation_id);
            slot.handle.clear();
            slot.stage = Stage::Mutating;
            None
        }
    };
    let outcome = if let Some(replay) = replay {
        replay
    } else {
        let mut lease = Lease {
            entry: entry.clone(),
            operation: request.operation_id,
            receipt: None,
            fallback: Fallback::Unknown,
            finished: false,
        };
        match state
            .client
            .prepare_bound_file(&entry.configured.binding)
            .await
        {
            Ok(prepared) => {
                let (ticket, stage) = match prepared {
                    PreparedFile::Delayed(receipt) => {
                        let stored =
                            serde_json::to_value(&receipt).map_err(|_| Error::Unavailable)?;
                        let delayed = stored
                            .get("delayed_id")
                            .and_then(|value| value.as_u64())
                            .filter(|value| *value > 0)
                            .ok_or(Error::Unavailable)?;
                        (Ticket { kind: 1, delayed }, Stage::Pending(receipt))
                    }
                    PreparedFile::Candidate(candidate) => (
                        Ticket {
                            kind: 2,
                            delayed: 0,
                        },
                        Stage::Candidate {
                            value: candidate,
                            revision: Uuid::new_v4(),
                            receipt: None,
                        },
                    ),
                };
                let handle = state
                    .receipts
                    .encode(request.operation_id, &entry.witness, ticket)?;
                lease.complete(stage, Some(handle.clone()))?;
                let slot = entry.slot.lock().map_err(|_| Error::Busy)?;
                PreparationSubmission::Accepted {
                    handle,
                    status: summary(&slot),
                }
            }
            Err(
                hito_debrid_all_debrid::Error::UnlockUnknown
                | hito_debrid_all_debrid::Error::SubmissionUnknown,
            ) => {
                lease.complete(Stage::Unknown, None)?;
                PreparationSubmission::SubmissionUnknown {}
            }
            Err(error) => {
                let reason = reason(error);
                let stage = if matches!(
                    reason,
                    PreparationReason::RateLimited | PreparationReason::ProviderUnavailable
                ) {
                    Stage::Retryable {
                        reason,
                        not_before: Instant::now() + Duration::from_secs(5),
                    }
                } else {
                    Stage::Unavailable
                };
                lease.complete(stage, None)?;
                PreparationSubmission::NotSubmitted {
                    reason,
                    retry_after_seconds: Some(5),
                }
            }
        }
    };
    let outcome = match outcome {
        PreparationSubmission::Accepted { handle, .. } => {
            let status = current_status(state, &entry, request.operation_id, &handle).await?;
            PreparationSubmission::Accepted { handle, status }
        }
        other => other,
    };
    Ok(PreparationSubmitResponse {
        operation_id: request.operation_id,
        recording_id: request.recording_id.clone(),
        source_id: request.source_id.clone(),
        submission: outcome,
    })
}

pub(crate) async fn status(
    state: &StateData,
    entry: Arc<Entry>,
    request: &PreparationStatusRequest,
) -> Result<PreparationStatusResponse, Error> {
    let ticket = state
        .receipts
        .decode(&request.handle, request.operation_id, &entry.witness)?;
    let pending = {
        let _claim = admit_operation(state, &entry, request.operation_id)?;
        let mut slot = entry.slot.lock().map_err(|_| Error::Busy)?;
        if slot.operation.is_none() {
            slot.operation = Some(request.operation_id);
            slot.handle = request.handle.clone();
            slot.stage = if ticket.kind == 1 {
                Stage::Pending(restore(&entry, ticket.delayed)?)
            } else {
                Stage::Unavailable
            };
        }
        if slot.operation != Some(request.operation_id) || slot.handle != request.handle {
            return Err(Error::NotFound);
        }
        if matches!(slot.stage, Stage::Pending(_)) {
            match std::mem::replace(&mut slot.stage, Stage::Polling) {
                Stage::Pending(receipt) => Some(receipt),
                _ => unreachable!(),
            }
        } else {
            None
        }
    };
    let result = match pending {
        None => current_status(state, &entry, request.operation_id, &request.handle).await?,
        Some(receipt) => {
            let mut lease = Lease {
                entry: entry.clone(),
                operation: request.operation_id,
                receipt: Some(receipt),
                fallback: Fallback::Pending,
                finished: false,
            };
            match state
                .client
                .poll_delayed(lease.receipt.as_mut().ok_or(Error::Unavailable)?)
                .await
            {
                Ok(DelayedPoll::Candidate(candidate)) => {
                    let receipt = lease.receipt.take().ok_or(Error::Unavailable)?;
                    lease.complete(
                        Stage::Candidate {
                            value: candidate,
                            revision: Uuid::new_v4(),
                            receipt: Some(receipt),
                        },
                        None,
                    )?;
                    current_status(state, &entry, request.operation_id, &request.handle).await?
                }
                Ok(DelayedPoll::Processing {
                    time_left_seconds, ..
                }) => {
                    let receipt = lease.receipt.take().ok_or(Error::Unavailable)?;
                    lease.complete(Stage::Pending(receipt), None)?;
                    preparing(Some(time_left_seconds))
                }
                Err(
                    hito_debrid_all_debrid::Error::SourceChanged
                    | hito_debrid_all_debrid::Error::ProviderFailed,
                ) => {
                    lease.complete(Stage::Unavailable, None)?;
                    PreparationStatus::Failed {
                        reason: PreparationReason::SourceUnavailable,
                    }
                }
                Err(error) => PreparationStatus::Blocked {
                    reason: reason(error),
                    retry_after_seconds: Some(5),
                },
            }
        }
    };
    Ok(PreparationStatusResponse {
        operation_id: request.operation_id,
        recording_id: request.recording_id.clone(),
        source_id: request.source_id.clone(),
        handle: request.handle.clone(),
        status: result,
    })
}

fn admit_operation<'a>(
    state: &'a StateData,
    entry: &Entry,
    operation: Uuid,
) -> Result<MutexGuard<'a, ()>, Error> {
    let claim = state.catalog.claims.lock().map_err(|_| Error::Busy)?;
    for other in &state.catalog.entries {
        if !std::ptr::eq(other.as_ref(), entry)
            && other.slot.lock().map_err(|_| Error::Busy)?.operation == Some(operation)
        {
            return Err(Error::Conflict);
        }
    }
    Ok(claim)
}

fn restore(entry: &Entry, delayed: u64) -> Result<DelayedFileReceipt, Error> {
    serde_json::from_value(
        serde_json::json!({"binding":entry.configured.binding,"delayed_id":delayed}),
    )
    .map_err(|_| Error::NotFound)
}
fn reason(error: hito_debrid_all_debrid::Error) -> PreparationReason {
    use hito_debrid_all_debrid::Error as Provider;
    match error {
        Provider::Status(401 | 403) | Provider::InvalidCredential => {
            PreparationReason::AuthorizationRequired
        }
        Provider::Busy | Provider::Status(429) => PreparationReason::RateLimited,
        Provider::SourceChanged => PreparationReason::SourceChanged,
        Provider::InvalidResponse | Provider::ResponseTooLarge => {
            PreparationReason::InvalidResponse
        }
        Provider::UnsupportedState | Provider::UnsupportedAudio => PreparationReason::Unsupported,
        Provider::ProviderFailed => PreparationReason::SourceUnavailable,
        Provider::ProviderRejected => PreparationReason::ProviderRestricted,
        _ => PreparationReason::ProviderUnavailable,
    }
}

pub(crate) struct CandidateLease {
    _lease: Lease,
}
pub(crate) fn take_candidate(
    entry: Arc<Entry>,
) -> Result<(GeneratedFileCandidate, CandidateLease), Error> {
    let (operation, candidate, receipt) = {
        let mut slot = entry.slot.lock().map_err(|_| Error::Busy)?;
        let operation = slot.operation.ok_or(Error::NotFound)?;
        if !matches!(slot.stage, Stage::Candidate { .. }) {
            return Err(Error::Unavailable);
        }
        let Stage::Candidate {
            value: candidate,
            receipt,
            ..
        } = std::mem::replace(&mut slot.stage, Stage::Opening)
        else {
            unreachable!()
        };
        (operation, candidate, receipt)
    };
    let fallback = if receipt.is_some() {
        Fallback::Pending
    } else {
        Fallback::Unavailable
    };
    Ok((
        candidate,
        CandidateLease {
            _lease: Lease {
                entry,
                operation,
                receipt,
                fallback,
                finished: false,
            },
        },
    ))
}

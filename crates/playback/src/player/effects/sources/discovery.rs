use super::{Demand, Key, Pending};
use crate::{PlayerError, SourceStop};
use hito_core::{Catalog, Error, ResolveSource, SourceResolutionOutcome};
use std::time::Duration;
use tokio::time::{Instant, sleep_until};
use uuid::Uuid;

const BUDGET: Duration = Duration::from_secs(15);
const DELAYS: [Duration; 2] = [Duration::from_millis(500), Duration::from_millis(1000)];

pub(super) enum Outcome {
    Ready,
    Exhausted,
    Cancelled,
    Deadline,
    Failed(PlayerError),
}

pub(super) struct Report {
    pub key: Key,
    pub demand: Uuid,
    pub outcome: Outcome,
}

pub(super) struct Discovery {
    stop: SourceStop,
    done: Pending<Report>,
}

impl Discovery {
    pub fn start(catalog: Catalog, demand: Demand) -> Self {
        let stop = SourceStop::new();
        let cancellation = stop.clone();
        let started = Instant::now();
        let done = Box::pin(async move {
            let outcome = run(catalog, &demand, cancellation, started).await;
            tracing::debug!(
                event = "player_source_discovery_finished",
                operation_id = %demand.id,
                selection = demand.key.selection,
                outcome = outcome.name(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Current entry source discovery settled"
            );
            Report {
                key: demand.key,
                demand: demand.id,
                outcome,
            }
        });
        Self { stop, done }
    }

    pub fn cancel(&self) {
        self.stop.request();
    }

    pub async fn finished(&mut self) -> Report {
        self.done.as_mut().await
    }
}

impl Outcome {
    fn name(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Exhausted => "exhausted",
            Self::Cancelled => "cancelled",
            Self::Deadline => "deadline",
            Self::Failed(_) => "failed",
        }
    }
}

async fn run(catalog: Catalog, demand: &Demand, stop: SourceStop, started: Instant) -> Outcome {
    let deadline = started + BUDGET;
    for attempt in 1..=3 {
        if stop.requested() {
            return Outcome::Cancelled;
        }
        if Instant::now() >= deadline {
            return Outcome::Deadline;
        }
        let request_id = Uuid::new_v4();
        let attempt_started = Instant::now();
        tracing::debug!(
            event = "player_source_discovery_attempt_started",
            operation_id = %demand.id,
            request_id = %request_id,
            attempt,
            selection = demand.key.selection,
            "Discovering current entry audio"
        );
        let resolved = catalog
            .resolve_source_until(
                &demand.actor,
                &demand.recording,
                ResolveSource { request_id },
                async {
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => {},
                        _ = sleep_until(deadline) => {},
                    }
                },
            )
            .await;
        let (outcome, reason, retry) = match resolved {
            _ if stop.requested() => (Outcome::Cancelled, "cancelled", false),
            Err(
                error @ (Error::Unauthorized | Error::Locked | Error::NotFound | Error::Forbidden),
            ) => {
                let reason = error.code();
                (Outcome::Failed(PlayerError::from(error)), reason, false)
            }
            _ if Instant::now() >= deadline => (Outcome::Deadline, "deadline", false),
            Ok(resolution) => match resolution.outcome {
                SourceResolutionOutcome::Ready => (Outcome::Ready, "ready", false),
                SourceResolutionOutcome::NoSuitableSource => {
                    (Outcome::Exhausted, "no_suitable_source", attempt < 3)
                }
                SourceResolutionOutcome::ProviderUnavailable => {
                    let retry =
                        attempt < 3 && resolution.failure.is_some_and(|failure| failure.retryable);
                    (Outcome::Exhausted, "provider_unavailable", retry)
                }
                SourceResolutionOutcome::ProtectedChoice => {
                    (Outcome::Exhausted, "protected_choice", false)
                }
                SourceResolutionOutcome::Cancelled => (Outcome::Cancelled, "cancelled", false),
            },
            Err(error) => {
                let reason = error.code();
                let outcome = match error {
                    Error::PluginCredentialsRequired(_)
                    | Error::ProviderBusy(_)
                    | Error::AuthorizationRequired(_)
                    | Error::OperationRejected(_)
                    | Error::UpstreamNotice(_) => Outcome::Exhausted,
                    error => Outcome::Failed(PlayerError::from(error)),
                };
                (outcome, reason, false)
            }
        };
        tracing::info!(
            event = "player_source_discovery_attempt_finished",
            operation_id = %demand.id,
            request_id = %request_id,
            attempt,
            selection = demand.key.selection,
            reason,
            outcome = if retry { "retry" } else { outcome.name() },
            elapsed_ms = attempt_started.elapsed().as_millis() as u64,
            total_elapsed_ms = started.elapsed().as_millis() as u64,
            "Current entry discovery attempt settled"
        );
        if !retry {
            return outcome;
        }
        let wake = Instant::now() + DELAYS[(attempt - 1) as usize];
        if wake >= deadline {
            return Outcome::Deadline;
        }
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Outcome::Cancelled,
            _ = sleep_until(deadline) => return Outcome::Deadline,
            _ = sleep_until(wake) => {},
        }
    }
    Outcome::Exhausted
}

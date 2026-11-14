use super::{Demand, Key, Pending};
use crate::{PlayerError, SourceStop};
use hito_core::{Catalog, Error, PlaybackFailureReason, ResolveSource, SourceResolutionOutcome};
use tokio::time::Instant;
use uuid::Uuid;

pub(super) enum Outcome {
    Ready,
    NoSuitable,
    ProviderUnavailable { retryable: bool },
    ProtectedChoice,
    Cancelled,
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
    pub fn start(
        catalog: Catalog,
        demand: Demand,
        attempt: u8,
        total_started: Option<Instant>,
    ) -> Self {
        let stop = SourceStop::new();
        let cancellation = stop.clone();
        let started = total_started.unwrap_or_else(Instant::now);
        let done = Box::pin(async move {
            let outcome = run(catalog, &demand, cancellation, attempt, started).await;
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
            Self::NoSuitable => "no_suitable_source",
            Self::ProviderUnavailable { .. } => "provider_unavailable",
            Self::ProtectedChoice => "protected_choice",
            Self::Cancelled => "cancelled",
            Self::Failed(_) => "failed",
        }
    }
}

async fn run(
    catalog: Catalog,
    demand: &Demand,
    stop: SourceStop,
    attempt: u8,
    started: Instant,
) -> Outcome {
    if stop.requested() {
        return Outcome::Cancelled;
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
                stop.cancelled().await;
            },
        )
        .await;
    let attempt_elapsed = attempt_started.elapsed().as_millis() as u64;
    let (outcome, reason) = match resolved {
        _ if stop.requested() => (Outcome::Cancelled, "cancelled"),
        Err(error @ (Error::Unauthorized | Error::Locked | Error::NotFound | Error::Forbidden)) => {
            let reason = error.code();
            (Outcome::Failed(PlayerError::from(error)), reason)
        }
        Ok(resolution) => match resolution.outcome {
            SourceResolutionOutcome::Ready => (Outcome::Ready, "ready"),
            SourceResolutionOutcome::NoSuitableSource => {
                (Outcome::NoSuitable, "no_suitable_source")
            }
            SourceResolutionOutcome::ProviderUnavailable => (
                Outcome::ProviderUnavailable {
                    retryable: resolution.failure.is_some_and(|failure| failure.retryable),
                },
                "provider_unavailable",
            ),
            SourceResolutionOutcome::ProtectedChoice => {
                (Outcome::ProtectedChoice, "protected_choice")
            }
            SourceResolutionOutcome::Cancelled => (Outcome::Cancelled, "cancelled"),
        },
        Err(error @ Error::ProviderBusy(_)) => {
            let reason = error.code();
            (Outcome::ProviderUnavailable { retryable: true }, reason)
        }
        Err(
            error @ Error::PlaybackProvider {
                reason: PlaybackFailureReason::ProviderUnavailable,
                ..
            },
        ) => {
            let reason = error.code();
            (Outcome::ProviderUnavailable { retryable: true }, reason)
        }
        Err(error) => {
            let reason = error.code();
            let outcome = match error {
                Error::PluginCredentialsRequired(_)
                | Error::AuthorizationRequired(_)
                | Error::OperationRejected(_)
                | Error::UpstreamNotice(_) => Outcome::ProtectedChoice,
                error => Outcome::Failed(PlayerError::from(error)),
            };
            (outcome, reason)
        }
    };
    tracing::info!(
        event = "player_source_discovery_attempt_finished",
        operation_id = %demand.id,
        request_id = %request_id,
        attempt,
        selection = demand.key.selection,
        reason,
        outcome = outcome.name(),
        elapsed_ms = attempt_elapsed,
        total_elapsed_ms = started.elapsed().as_millis() as u64,
        "Current entry discovery attempt settled"
    );
    outcome
}

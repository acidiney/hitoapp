use hito_core::{
    Actor, Catalog, PrepareSource, ResolveSource, Source, SourcePreparationPhase,
    SourcePreparationStatus,
};
use std::{
    future::Future,
    pin::Pin,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::watch, time::Instant};
use uuid::Uuid;

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub(super) const PREPARATION_BOUND: Duration = Duration::from_secs(90);
pub(super) const BOUNDARY_WAIT: Duration = Duration::from_secs(10);
const DISCOVERY_BOUND: Duration = Duration::from_secs(15);
const MIN_CHECK: Duration = Duration::from_secs(5);
const MAX_STATUS_FAILURES: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Outcome {
    Ready,
    Unavailable,
}

pub(super) struct Deferral {
    stop: watch::Sender<bool>,
    done: Pending<Outcome>,
}

impl Deferral {
    pub fn start(catalog: Catalog, actor: Actor, recording: Uuid) -> Self {
        let (stop, stopped) = watch::channel(false);
        let work = Work {
            catalog,
            actor,
            recording: recording.to_string(),
            stopped,
            started: Instant::now(),
        };
        Self {
            stop,
            done: Box::pin(work.run()),
        }
    }

    pub async fn finished(&mut self) -> Outcome {
        self.done.as_mut().await
    }

    pub fn cancel(self) -> Pending<()> {
        self.stop.send_replace(true);
        let done = self.done;
        Box::pin(async move {
            done.await;
        })
    }
}

struct Work {
    catalog: Catalog,
    actor: Actor,
    recording: String,
    stopped: watch::Receiver<bool>,
    started: Instant,
}

impl Work {
    fn cancelled(&self) -> Pending<()> {
        let mut stopped = self.stopped.clone();
        let discovery_end = self.started + DISCOVERY_BOUND;
        Box::pin(async move {
            tokio::select! {
                _ = stopped.wait_for(|stop| *stop) => {},
                _ = tokio::time::sleep_until(discovery_end) => {},
            }
        })
    }

    fn stopping(&self) -> bool {
        *self.stopped.borrow()
    }

    async fn sources(&self) -> Option<Vec<Source>> {
        self.catalog
            .sources(&self.actor, &self.recording)
            .await
            .ok()
    }

    async fn run(self) -> Outcome {
        let outcome = self.settle().await;
        tracing::info!(
            event = "player_deferred_source_finished",
            ready = outcome == Outcome::Ready,
            cancelled = self.stopping(),
            elapsed_ms = self.started.elapsed().as_millis() as u64,
            "Next entry source preparation finished"
        );
        outcome
    }

    async fn settle(&self) -> Outcome {
        let Some(sources) = self.sources().await else {
            return Outcome::Unavailable;
        };
        if sources.iter().any(|source| source.ready) {
            return Outcome::Ready;
        }
        if let Err(hito_core::Error::Unauthorized) = self
            .catalog
            .refresh_playback_sources_until(&self.actor, &self.recording, self.cancelled())
            .await
        {
            return Outcome::Unavailable;
        }
        if self.stopping() {
            return Outcome::Unavailable;
        }
        if let Some(sources) = self.sources().await
            && sources.iter().any(|source| source.ready)
        {
            return Outcome::Ready;
        }
        let request = ResolveSource {
            request_id: Uuid::new_v4(),
        };
        let resolved = self
            .catalog
            .resolve_source_until(&self.actor, &self.recording, request, self.cancelled())
            .await;
        if self.stopping() {
            return Outcome::Unavailable;
        }
        let sources = match resolved {
            Ok(resolution) => resolution.sources,
            Err(_) => match self.sources().await {
                Some(sources) => sources,
                None => return Outcome::Unavailable,
            },
        };
        if sources.iter().any(|source| source.ready) {
            return Outcome::Ready;
        }
        let Some(waiting) = sources.into_iter().find(|source| source.can_prepare) else {
            tracing::debug!(
                event = "player_deferred_source_unavailable",
                "No preparable source exists for the next entry"
            );
            return Outcome::Unavailable;
        };
        self.prepare(&waiting.id).await
    }

    async fn prepare(&self, source: &str) -> Outcome {
        let request = PrepareSource {
            request_id: Uuid::new_v4(),
        };
        tracing::info!(
            event = "player_deferred_source_requested",
            request_id = %request.request_id,
            "Next entry source preparation requested"
        );
        if self
            .catalog
            .prepare_source(&self.actor, &self.recording, source, request)
            .await
            .is_err()
        {
            return match self.sources().await {
                Some(sources) if sources.iter().any(|source| source.ready) => Outcome::Ready,
                _ => {
                    tracing::warn!(
                        event = "player_deferred_source_rejected",
                        "Next entry source preparation was not admitted"
                    );
                    Outcome::Unavailable
                }
            };
        }
        let deadline = self.started + PREPARATION_BOUND;
        let mut stopped = self.stopped.clone();
        let mut failures = 0;
        loop {
            let delay = match self
                .catalog
                .source_preparation(&self.actor, &self.recording, source)
                .await
            {
                Ok(status) => {
                    failures = 0;
                    tracing::debug!(
                        event = "player_deferred_source_checked",
                        phase = phase_name(&status.phase),
                        "Next entry source preparation checked"
                    );
                    match status.phase {
                        SourcePreparationPhase::Ready => return Outcome::Ready,
                        SourcePreparationPhase::Blocked
                        | SourcePreparationPhase::Failed
                        | SourcePreparationPhase::Cancelled
                        | SourcePreparationPhase::SubmissionUnknown => {
                            tracing::warn!(
                                event = "player_deferred_source_stopped",
                                "Next entry source preparation stopped"
                            );
                            return Outcome::Unavailable;
                        }
                        _ => until_check(&status),
                    }
                }
                Err(_) => {
                    failures += 1;
                    if failures >= MAX_STATUS_FAILURES {
                        tracing::warn!(
                            event = "player_deferred_source_unchecked",
                            "Next entry source preparation could not be checked"
                        );
                        return Outcome::Unavailable;
                    }
                    MIN_CHECK
                }
            };
            if self.stopping() || Instant::now() + delay >= deadline {
                return Outcome::Unavailable;
            }
            tokio::select! {
                _ = stopped.wait_for(|stop| *stop) => return Outcome::Unavailable,
                _ = tokio::time::sleep(delay) => {},
            }
        }
    }
}

fn until_check(status: &SourcePreparationStatus) -> Duration {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let due = status.next_check_at.map_or(Duration::ZERO, |due| {
        Duration::from_secs((due - now).max(0) as u64)
    });
    due.max(MIN_CHECK)
}

fn phase_name(phase: &SourcePreparationPhase) -> &'static str {
    match phase {
        SourcePreparationPhase::Queued => "queued",
        SourcePreparationPhase::Submitting => "submitting",
        SourcePreparationPhase::Preparing => "preparing",
        SourcePreparationPhase::Ready => "ready",
        SourcePreparationPhase::Blocked => "blocked",
        SourcePreparationPhase::Failed => "failed",
        SourcePreparationPhase::SubmissionUnknown => "submission_unknown",
        SourcePreparationPhase::Cancelled => "cancelled",
    }
}

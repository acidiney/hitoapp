use super::{ChangeNotice, Changes, Registration, ResyncReason, Signal};
use crate::{Actor, Error};
use std::time::{Duration, Instant};
use tokio::{
    sync::broadcast::error::RecvError,
    time::{Interval, MissedTickBehavior},
};
use tracing::Instrument;
use uuid::Uuid;

const SESSION_CHECK: Duration = Duration::from_secs(30);
const SESSION_CHECK_DEADLINE: Duration = Duration::from_secs(1);

pub struct ChangeStream {
    changes: Changes,
    actor: Actor,
    registration: Registration,
    pending: Option<ChangeNotice>,
    checks: Interval,
    span: tracing::Span,
    dispatch: tracing::Dispatch,
    started: Instant,
    delivered: u64,
    resyncs: u64,
    outcome: &'static str,
}

enum Wake {
    Signal(Result<Signal, RecvError>),
    Check,
}

impl ChangeStream {
    pub(super) fn new(
        changes: Changes,
        actor: Actor,
        registration: Registration,
    ) -> Self {
        let span = tracing::debug_span!(
            "change_stream",
            stream_id = %Uuid::new_v4(),
            account_id = actor.id()
        );
        span.in_scope(|| {
            tracing::debug!(event = "change_stream_connected", "Change stream connected")
        });
        let mut checks =
            tokio::time::interval_at(tokio::time::Instant::now() + SESSION_CHECK, SESSION_CHECK);
        checks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Self {
            changes,
            actor,
            registration,
            pending: Some(ChangeNotice::Resync {
                reason: ResyncReason::Connected,
            }),
            checks,
            span,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            started: Instant::now(),
            delivered: 0,
            resyncs: 0,
            outcome: "disconnected",
        }
    }

    pub async fn next(&mut self) -> Option<ChangeNotice> {
        let span = self.span.clone();
        self.advance().instrument(span).await
    }

    async fn advance(&mut self) -> Option<ChangeNotice> {
        if let Some(notice) = self.pending.take() {
            return Some(notice);
        }
        loop {
            let receiver = self.registration.receiver.as_mut()?;
            let wake = tokio::select! {
                signal = receiver.recv() => Wake::Signal(signal),
                _ = self.checks.tick() => Wake::Check,
            };
            match wake {
                Wake::Signal(Ok(Signal::Notice(notice))) => {
                    if self.session_ended().await {
                        return None;
                    }
                    self.delivered += 1;
                    tracing::trace!(
                        event = "change_notice_delivered",
                        kind = notice.kind(),
                        "Change notice delivered"
                    );
                    return Some(notice);
                }
                Wake::Signal(Ok(Signal::SessionsChanged)) | Wake::Check => {
                    if self.session_ended().await {
                        return None;
                    }
                }
                Wake::Signal(Err(RecvError::Lagged(skipped))) => {
                    self.resyncs += 1;
                    tracing::debug!(
                        event = "change_stream_lagged",
                        outcome = "resync",
                        skipped,
                        "Change stream fell behind and was asked to resynchronize"
                    );
                    if self.session_ended().await {
                        return None;
                    }
                    return Some(ChangeNotice::Resync {
                        reason: ResyncReason::Lagged,
                    });
                }
                Wake::Signal(Err(RecvError::Closed)) => {
                    self.outcome = "closed";
                    self.registration.close();
                    return None;
                }
            }
        }
    }

    async fn session_ended(&mut self) -> bool {
        let checked = tokio::time::timeout(
            SESSION_CHECK_DEADLINE,
            self.actor.check_session(&self.changes.store),
        )
        .await;
        let outcome = match checked {
            Ok(Ok(())) => return false,
            Ok(Err(Error::Unauthorized)) => "session_ended",
            Ok(Err(Error::Locked)) => "session_locked",
            Ok(Err(_)) => "session_check_failed",
            Err(_) => "session_check_timed_out",
        };
        self.outcome = outcome;
        self.registration.close();
        true
    }
}

impl Drop for ChangeStream {
    fn drop(&mut self) {
        self.registration.close();
        tracing::dispatcher::with_default(&self.dispatch, || {
            self.span.in_scope(|| {
                tracing::debug!(
                    event = "change_stream_disconnected",
                    outcome = self.outcome,
                    delivered = self.delivered,
                    resyncs = self.resyncs,
                    elapsed_ms = self.started.elapsed().as_millis() as u64,
                    "Change stream disconnected"
                );
            })
        });
    }
}

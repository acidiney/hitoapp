use std::{
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};
use tokio::{
    sync::watch,
    time::{MissedTickBehavior, interval},
};

const POLL_PERIOD: Duration = Duration::from_secs(1);

pub(super) type StopSignal = Pin<Box<dyn Future<Output = ()> + Send>>;

#[derive(Clone, Copy)]
pub(super) struct Event {
    pub name: &'static str,
    pub message: &'static str,
}

#[derive(Clone, Copy)]
pub(super) enum Reporting {
    Claimed,
    ClaimedWithOutcome,
    Progressed,
}

#[derive(Clone, Copy)]
pub(super) struct Worker {
    pub started: Event,
    pub polled: Event,
    pub failed: Event,
    pub stopped: Event,
    pub reporting: Reporting,
}

pub(super) async fn run<Poll, Pending>(
    worker: Worker,
    mut stopped: watch::Receiver<bool>,
    mut poll: Poll,
) where
    Poll: FnMut(StopSignal) -> Pending,
    Pending: Future<Output = hito_core::Result<bool>>,
{
    tracing::info!(event = worker.started.name, "{}", worker.started.message);
    let mut ticks = interval(POLL_PERIOD);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    while !*stopped.borrow() {
        tokio::select! {
            biased;
            _ = stopped.changed() => break,
            _ = ticks.tick() => {}
        }
        let started = Instant::now();
        let result = poll(stop_signal(stopped.clone())).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(advanced) => log_polled(worker, advanced, elapsed_ms),
            Err(error) => log_failed(worker, error.code(), elapsed_ms),
        }
    }
    log_stopped(worker);
}

fn stop_signal(mut stopped: watch::Receiver<bool>) -> StopSignal {
    Box::pin(async move {
        let _ = stopped.wait_for(|requested| *requested).await;
    })
}

fn log_polled(worker: Worker, advanced: bool, elapsed_ms: u64) {
    let Event { name, message } = worker.polled;
    match worker.reporting {
        Reporting::Claimed => {
            tracing::debug!(event = name, claimed = advanced, elapsed_ms, "{message}")
        }
        Reporting::ClaimedWithOutcome => tracing::debug!(
            event = name,
            claimed = advanced,
            outcome = if advanced { "processed" } else { "idle" },
            elapsed_ms,
            "{message}"
        ),
        Reporting::Progressed => {
            tracing::debug!(event = name, progressed = advanced, elapsed_ms, "{message}")
        }
    }
}

fn log_failed(worker: Worker, error_kind: &'static str, elapsed_ms: u64) {
    let Event { name, message } = worker.failed;
    match worker.reporting {
        Reporting::ClaimedWithOutcome => tracing::error!(
            event = name,
            error_kind,
            outcome = "failed",
            elapsed_ms,
            "{message}"
        ),
        Reporting::Claimed | Reporting::Progressed => {
            tracing::error!(event = name, error_kind, elapsed_ms, "{message}")
        }
    }
}

fn log_stopped(worker: Worker) {
    let Event { name, message } = worker.stopped;
    match worker.reporting {
        Reporting::ClaimedWithOutcome => {
            tracing::info!(event = name, outcome = "stopped", "{message}")
        }
        Reporting::Claimed | Reporting::Progressed => tracing::info!(event = name, "{message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hito_core::test_support::hang_guard::hang_guard;
    use hito_testkit::logs::LogCapture;
    use std::sync::Arc;
    use tokio::sync::Notify;
    use tracing::instrument::WithSubscriber;

    const WORKER: Worker = Worker {
        started: Event {
            name: "probe_worker_started",
            message: "Probe worker started",
        },
        polled: Event {
            name: "probe_worker_polled",
            message: "Probe worker checked pending work",
        },
        failed: Event {
            name: "probe_worker_failed",
            message: "Probe worker failed",
        },
        stopped: Event {
            name: "probe_worker_stopped",
            message: "Probe worker stopped",
        },
        reporting: Reporting::Claimed,
    };

    #[tokio::test]
    async fn a_stop_signal_arriving_mid_poll_ends_the_loop_and_logs_one_stop() {
        let capture = LogCapture::unbounded();
        let (stop, stopped) = watch::channel(false);
        let entered = Arc::new(Notify::new());
        let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let worker = {
            let entered = entered.clone();
            let polls = polls.clone();
            run(WORKER, stopped, move |stop_signal| {
                let entered = entered.clone();
                let polls = polls.clone();
                async move {
                    polls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    entered.notify_one();
                    stop_signal.await;
                    Ok(false)
                }
            })
        };
        let signalling = async {
            entered.notified().await;
            stop.send_replace(true);
        };
        hang_guard("the worker must stop mid-poll", async {
            tokio::join!(worker, signalling);
        })
        .with_subscriber(capture.dispatch("hito_server=trace"))
        .await;
        let events = capture.events();
        let named = |name: &str| {
            events
                .iter()
                .filter(|event| event["fields"]["event"] == name)
                .count()
        };
        assert_eq!(polls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(named("probe_worker_started"), 1);
        assert_eq!(named("probe_worker_polled"), 1);
        assert_eq!(named("probe_worker_stopped"), 1);
    }
}

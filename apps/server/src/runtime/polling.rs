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
    use crate::runtime::{
        artwork, community, downloads, follows, indexing, matching, podcasts, source_preparation,
    };
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

    async fn run_one_poll(
        worker: Worker,
        result: fn() -> hito_core::Result<bool>,
    ) -> Vec<serde_json::Value> {
        let capture = LogCapture::unbounded();
        let (stop, stopped) = watch::channel(false);
        hang_guard(
            "one poll must finish",
            run(worker, stopped, move |_| {
                stop.send_replace(true);
                std::future::ready(result())
            }),
        )
        .with_subscriber(capture.dispatch("hito_server=trace"))
        .await;
        capture.events()
    }

    async fn polled_fields(worker: Worker, advanced: bool) -> serde_json::Value {
        let events = if advanced {
            run_one_poll(worker, || Ok(true)).await
        } else {
            run_one_poll(worker, || Ok(false)).await
        };
        events
            .into_iter()
            .find(|event| event["fields"]["event"] == worker.polled.name)
            .expect("the poll must be logged")["fields"]
            .clone()
    }

    #[test]
    fn every_worker_keeps_its_event_names_and_messages() {
        let expected: [(Worker, [(&str, &str); 4]); 10] = [
            (
                artwork::ARTWORK,
                [
                    ("artwork_worker_started", "Artwork worker started"),
                    (
                        "artwork_worker_polled",
                        "Artwork worker checked pending work",
                    ),
                    ("artwork_worker_failed", "Artwork worker failed"),
                    ("artwork_worker_stopped", "Artwork worker stopped"),
                ],
            ),
            (
                artwork::COVER_ART,
                [
                    ("cover_art_worker_started", "Cover art worker started"),
                    (
                        "cover_art_worker_polled",
                        "Cover art worker checked pending work",
                    ),
                    ("cover_art_worker_failed", "Cover art worker failed"),
                    ("cover_art_worker_stopped", "Cover art worker stopped"),
                ],
            ),
            (
                community::REPORTS,
                [
                    ("community_worker_started", "Community worker started"),
                    (
                        "community_worker_polled",
                        "Community worker checked pending reports",
                    ),
                    ("community_worker_failed", "Community worker failed"),
                    ("community_worker_stopped", "Community worker stopped"),
                ],
            ),
            (
                downloads::TRANSFER,
                [
                    ("download_worker_started", "Download worker started"),
                    (
                        "download_worker_polled",
                        "Download worker checked pending work",
                    ),
                    ("download_worker_failed", "Download worker failed"),
                    ("download_worker_stopped", "Download worker stopped"),
                ],
            ),
            (
                downloads::RECONCILIATION,
                [
                    (
                        "download_reconciliation_started",
                        "Playlist download reconciliation started",
                    ),
                    (
                        "download_reconciliation_polled",
                        "Playlist download reconciliation checked work",
                    ),
                    (
                        "download_reconciliation_failed",
                        "Playlist download reconciliation failed",
                    ),
                    (
                        "download_reconciliation_stopped",
                        "Playlist download reconciliation stopped",
                    ),
                ],
            ),
            (
                follows::FOLLOW,
                [
                    ("follow_worker_started", "Playlist follow worker started"),
                    (
                        "follow_worker_polled",
                        "Playlist follow worker checked pending work",
                    ),
                    ("follow_worker_failed", "Playlist follow worker failed"),
                    ("follow_worker_stopped", "Playlist follow worker stopped"),
                ],
            ),
            (
                indexing::INDEXING,
                [
                    ("worker_started", "Indexing worker started"),
                    ("worker_polled", "Indexing worker checked pending work"),
                    ("worker_failed", "Background worker failed"),
                    ("worker_stopped", "Indexing worker stopped"),
                ],
            ),
            (
                matching::MATCHING,
                [
                    ("matching_worker_started", "Artist matching worker started"),
                    (
                        "matching_worker_polled",
                        "Artist matching worker checked pending work",
                    ),
                    ("matching_worker_failed", "Artist matching worker failed"),
                    ("matching_worker_stopped", "Artist matching worker stopped"),
                ],
            ),
            (
                podcasts::REFRESH,
                [
                    ("podcast_worker_started", "Podcast refresh worker started"),
                    (
                        "podcast_worker_polled",
                        "Podcast refresh worker checked pending work",
                    ),
                    ("podcast_worker_failed", "Podcast refresh worker failed"),
                    ("podcast_worker_stopped", "Podcast refresh worker stopped"),
                ],
            ),
            (
                source_preparation::PREPARATION,
                [
                    (
                        "source_preparation_worker_started",
                        "Source preparation worker started",
                    ),
                    (
                        "source_preparation_worker_polled",
                        "Source preparation worker checked pending work",
                    ),
                    (
                        "source_preparation_worker_failed",
                        "Source preparation worker failed",
                    ),
                    (
                        "source_preparation_worker_stopped",
                        "Source preparation worker stopped",
                    ),
                ],
            ),
        ];
        for (worker, events) in expected {
            let actual = [worker.started, worker.polled, worker.failed, worker.stopped]
                .map(|event| (event.name, event.message));
            assert_eq!(actual, events);
        }
    }

    #[tokio::test]
    async fn a_claimed_poll_logs_whether_work_was_claimed() {
        let fields = polled_fields(WORKER, true).await;
        assert_eq!(fields["claimed"], true);
        assert!(fields["elapsed_ms"].is_u64());
        assert!(fields.get("outcome").is_none());
        assert_eq!(polled_fields(WORKER, false).await["claimed"], false);
    }

    #[tokio::test]
    async fn a_claimed_poll_with_outcome_logs_processed_or_idle() {
        let worker = Worker {
            reporting: Reporting::ClaimedWithOutcome,
            ..WORKER
        };
        let busy = polled_fields(worker, true).await;
        assert_eq!(
            (busy["claimed"].clone(), busy["outcome"].clone()),
            (true.into(), "processed".into())
        );
        let idle = polled_fields(worker, false).await;
        assert_eq!(
            (idle["claimed"].clone(), idle["outcome"].clone()),
            (false.into(), "idle".into())
        );
    }

    #[tokio::test]
    async fn a_progressed_poll_logs_whether_work_advanced() {
        let worker = Worker {
            reporting: Reporting::Progressed,
            ..WORKER
        };
        let fields = polled_fields(worker, true).await;
        assert_eq!(fields["progressed"], true);
        assert!(fields.get("claimed").is_none());
        assert_eq!(polled_fields(worker, false).await["progressed"], false);
    }

    #[tokio::test]
    async fn a_failed_poll_logs_the_error_kind_and_elapsed_time() {
        let events =
            run_one_poll(WORKER, || Err(hito_core::Error::Internal("hidden".into()))).await;
        let failed = events
            .iter()
            .find(|event| event["fields"]["event"] == "probe_worker_failed")
            .expect("the failure must be logged");
        assert_eq!(failed["level"], "ERROR");
        assert_eq!(failed["fields"]["error_kind"], "internal");
        assert!(failed["fields"]["elapsed_ms"].is_u64());
        assert!(!serde_json::to_string(&events).unwrap().contains("hidden"));
    }

    #[tokio::test]
    async fn a_failed_poll_with_outcome_also_logs_a_failed_outcome() {
        let worker = Worker {
            reporting: Reporting::ClaimedWithOutcome,
            ..WORKER
        };
        let events = run_one_poll(worker, || Err(hito_core::Error::NotFound)).await;
        let failed = events
            .iter()
            .find(|event| event["fields"]["event"] == "probe_worker_failed")
            .expect("the failure must be logged");
        assert_eq!(failed["fields"]["outcome"], "failed");
        assert_eq!(failed["fields"]["error_kind"], "not_found");
    }

    #[tokio::test]
    async fn no_poll_runs_when_stop_is_already_set() {
        let capture = LogCapture::unbounded();
        let (_stop, stopped) = watch::channel(true);
        let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = polls.clone();
        hang_guard("a stopped worker must return", async move {
            run(WORKER, stopped, move |_| {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::future::ready(Ok(false))
            })
            .await;
        })
        .with_subscriber(capture.dispatch("hito_server=trace"))
        .await;
        let names: Vec<_> = capture
            .events()
            .iter()
            .map(|event| event["fields"]["event"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(polls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(names, ["probe_worker_started", "probe_worker_stopped"]);
    }
}

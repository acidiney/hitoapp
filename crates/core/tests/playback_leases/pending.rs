use futures_util::FutureExt;
use hito_core::{
    AudioRequest, BeginPlaybackLease, Error, OpenedAudio,
    test_support::{hang_guard::hang_guard, provider_audio::AudioLibrary},
};
use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};
use tokio::{sync::Notify, task::JoinHandle};
use tracing::{Subscriber, instrument::WithSubscriber};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

#[derive(Clone)]
struct TicketSnapshot(Arc<Notify>);

impl<S: Subscriber> Layer<S> for TicketSnapshot {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        #[derive(Default)]
        struct Fields {
            is_claim_decision: bool,
            is_ticket_snapshot: bool,
            ticket_present: Option<bool>,
        }

        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}

            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                match field.name() {
                    "event" => {
                        self.is_claim_decision = value == "playback_lease_warm_claim_decision"
                    }
                    "stage" => self.is_ticket_snapshot = value == "ticket_snapshot",
                    _ => {}
                }
            }

            fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
                if field.name() == "ticket_present" {
                    self.ticket_present = Some(value);
                }
            }
        }

        let mut fields = Fields::default();
        event.record(&mut fields);
        if fields.is_claim_decision
            && fields.is_ticket_snapshot
            && fields.ticket_present == Some(false)
        {
            self.0.notify_one();
        }
    }
}

#[tokio::test]
async fn pending_provider_headers_report_a_missing_ticket_before_remote_fallback() {
    let (f, header_gate, held_tail, requests_before_lease) =
        hang_guard("owned pending provider audio setup", async {
            let f = AudioLibrary::start().await;
            f.ready().await;
            let header_gate = f.server.hold("v1/audio").await;
            let held_tail = f.server.hold_audio_tail(128);
            let requests_before_lease = f.server.requests("v1/audio").await.len();
            (f, header_gate, held_tail, requests_before_lease)
        })
        .await;

    let ticket_snapshot_absent = Arc::new(Notify::new());
    let subscriber =
        tracing_subscriber::registry().with(TicketSnapshot(ticket_snapshot_absent.clone()));
    let mut open_task: Option<JoinHandle<hito_core::Result<OpenedAudio>>> = None;
    let mut lease_created = false;
    let mut header_request_started = false;
    let mut ticket_was_absent = false;
    let mut response_shape_is_expected = false;
    let mut prefix_was_available_while_tail_held = false;
    let mut playback_completed = false;
    let mut playback_bytes = Vec::new();
    let mut request_count = None;
    let mut requests_are_gets = false;
    let mut requests_have_no_range = false;
    let mut download_is_absent = false;
    let mut source_is_unchanged = false;

    let attempt = AssertUnwindSafe(async {
        if let Ok(Ok(lease)) = tokio::time::timeout(
            Duration::from_secs(5),
            f.hito
                .playback
                .begin_lease(
                    &f.actor,
                    BeginPlaybackLease {
                        recording_id: f.source.recording_id.clone(),
                        source_id: f.source.id.clone(),
                    },
                )
                .with_subscriber(subscriber.clone()),
        )
        .await
        {
            lease_created = true;
            header_request_started =
                tokio::time::timeout(Duration::from_secs(2), header_gate.entered())
                    .await
                    .is_ok();

            if header_request_started {
                let playback = f.hito.playback.clone();
                let actor = f.actor.clone();
                open_task = Some(tokio::spawn(
                    async move {
                        playback
                            .open_lease_audio(&actor, lease.id, AudioRequest::default())
                            .await
                    }
                    .with_subscriber(subscriber.clone()),
                ));
                ticket_was_absent =
                    tokio::time::timeout(Duration::from_secs(2), ticket_snapshot_absent.notified())
                        .await
                        .is_ok();

                if ticket_was_absent {
                    header_gate.release();
                    header_gate.release();
                    let opened = tokio::time::timeout(
                        Duration::from_secs(5),
                        open_task.as_mut().expect("owned open task"),
                    )
                    .await;
                    if opened.is_ok() {
                        open_task = None;
                    }
                    if let Ok(Ok(Ok(OpenedAudio::Content(mut audio)))) = opened {
                        response_shape_is_expected = audio.content_type == "audio/wav"
                            && audio.length == Some(f.original.len() as u64)
                            && audio.total_length == Some(f.original.len() as u64);
                        if let Some(mut body) = audio.body.take() {
                            let prefix = tokio::time::timeout(Duration::from_secs(5), async {
                                let mut prefix = Vec::new();
                                while prefix.len() < 128 {
                                    match body.chunk().await? {
                                        Some(bytes) => prefix.extend_from_slice(&bytes),
                                        None => return Err(Error::NotFound),
                                    }
                                }
                                Ok::<_, Error>(prefix)
                            })
                            .await;
                            if let Ok(Ok(prefix)) = prefix {
                                prefix_was_available_while_tail_held = prefix
                                    .get(..128)
                                    .is_some_and(|bytes| bytes == &f.original[..128])
                                    && !held_tail.released();
                                playback_bytes.extend_from_slice(&prefix);
                                held_tail.release();
                                let remaining =
                                    tokio::time::timeout(Duration::from_secs(5), async {
                                        let mut remaining = Vec::new();
                                        while let Some(bytes) = body.chunk().await? {
                                            remaining.extend_from_slice(&bytes);
                                        }
                                        Ok::<_, Error>(remaining)
                                    })
                                    .await;
                                if let Ok(Ok(remaining)) = remaining {
                                    playback_bytes.extend_from_slice(&remaining);
                                    playback_completed = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Ok(requests) =
            tokio::time::timeout(Duration::from_secs(2), f.server.requests("v1/audio")).await
        {
            request_count = Some(requests.len());
            requests_are_gets = requests.iter().all(|(_, query)| {
                query.get("_method").and_then(serde_json::Value::as_str) == Some("GET")
            });
            requests_have_no_range = requests
                .iter()
                .all(|(headers, _)| headers.get("range").is_none());
        }
        download_is_absent = matches!(
            tokio::time::timeout(
                Duration::from_secs(2),
                f.hito.downloads.status(&f.actor, &f.source.recording_id),
            )
            .await,
            Ok(Err(Error::NotFound))
        );
        source_is_unchanged =
            std::fs::read(f.music.join("original.wav")).is_ok_and(|bytes| bytes == f.original);
    })
    .catch_unwind()
    .await;

    header_gate.release();
    header_gate.release();
    held_tail.release();
    if let Some(mut task) = open_task.take()
        && tokio::time::timeout(Duration::from_secs(5), &mut task)
            .await
            .is_err()
    {
        task.abort();
        let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
    }
    let playback_closed = matches!(
        tokio::time::timeout(Duration::from_secs(5), f.hito.playback.close()).await,
        Ok(Ok(()))
    );
    let replicas_closed = matches!(
        tokio::time::timeout(Duration::from_secs(5), f.hito.replicas.close()).await,
        Ok(Ok(()))
    );
    let replication_closed = matches!(
        tokio::time::timeout(Duration::from_secs(5), f.hito.catalog_replication.close()).await,
        Ok(Ok(()))
    );
    let server_closed = tokio::time::timeout(Duration::from_secs(5), f.server.shutdown())
        .await
        .is_ok();

    if let Err(panic) = attempt {
        std::panic::resume_unwind(panic);
    }

    eprintln!(
        "playback_access_pending_diagnostic stage=ticket_snapshot ticket_absent={} audio_requests={} all_get={} no_range={} prefix_bytes={} playback_bytes={} playback_completed={}",
        ticket_was_absent,
        request_count.unwrap_or(0),
        requests_are_gets,
        requests_have_no_range,
        playback_bytes.len().min(128),
        playback_bytes.len(),
        playback_completed
    );
    assert!(playback_closed && replicas_closed && replication_closed && server_closed);
    assert!(lease_created);
    assert!(header_request_started);
    assert!(ticket_was_absent);
    assert!(response_shape_is_expected);
    assert!(prefix_was_available_while_tail_held);
    assert!(playback_completed);
    assert_eq!(playback_bytes, f.original);
    assert_eq!(request_count, Some(requests_before_lease + 1));
    assert!(requests_are_gets);
    assert!(requests_have_no_range);
    assert!(download_is_absent);
    assert!(source_is_unchanged);
}

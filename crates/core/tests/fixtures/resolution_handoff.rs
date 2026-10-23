use crate::{
    fixture::Account,
    provider::{Event, Provider},
};
use hito_core::{Error, ResolveSource, Services, SourceResolution, SourceResolutionOutcome};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Notify, oneshot},
    task::JoinHandle,
};
use tracing::{Subscriber, instrument::WithSubscriber};
use tracing_subscriber::{Layer, layer::Context, prelude::*};
use uuid::Uuid;

#[derive(Clone, Default)]
struct Reserved(Arc<Notify>);

impl<S: Subscriber> Layer<S> for Reserved {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        struct Marker(bool);
        impl tracing::field::Visit for Marker {
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "event" && value == "source_resolution_handoff_waiting" {
                    self.0 = true;
                }
            }
        }
        let mut marker = Marker(false);
        event.record(&mut marker);
        if marker.0 {
            self.0.notify_one();
        }
    }
}

pub struct Call {
    task: Option<JoinHandle<hito_core::Result<SourceResolution>>>,
    cancel: Option<oneshot::Sender<()>>,
    cancellation_observed: Arc<Notify>,
    reserved: Reserved,
}

impl Call {
    pub fn start(hito: &Services, account: &Account, request_id: Uuid) -> Self {
        let catalog = hito.catalog.clone();
        let actor = account.actor.clone();
        let recording = account.recording.clone();
        let (cancel, cancelled) = oneshot::channel();
        let cancellation_observed = Arc::new(Notify::new());
        let observed = cancellation_observed.clone();
        let reserved = Reserved::default();
        let subscriber = tracing_subscriber::registry().with(reserved.clone());
        let task = tokio::spawn(
            async move {
                catalog
                    .resolve_source_until(
                        &actor,
                        &recording,
                        ResolveSource { request_id },
                        async move {
                            let _ = cancelled.await;
                            observed.notify_one();
                        },
                    )
                    .await
            }
            .with_subscriber(subscriber),
        );
        Self {
            task: Some(task),
            cancel: Some(cancel),
            cancellation_observed,
            reserved,
        }
    }

    pub fn request_cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }

    pub async fn cancel(&mut self) {
        self.request_cancel();
        tokio::time::timeout(
            Duration::from_secs(3),
            self.cancellation_observed.notified(),
        )
        .await
        .expect("the owned caller must observe supplied cancellation");
    }

    pub async fn reserved(&self) {
        tokio::time::timeout(Duration::from_secs(3), self.reserved.0.notified())
            .await
            .expect("the public successor must reserve the cancellation handoff");
    }

    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub async fn settle(
        &mut self,
        deadline: Duration,
        message: &'static str,
    ) -> hito_core::Result<SourceResolution> {
        let result = tokio::time::timeout(
            deadline,
            self.task.as_mut().expect("the owned caller must exist"),
        )
        .await
        .expect(message);
        self.task.take();
        result.expect("the owned caller must join without panic")
    }

    pub async fn finish(&mut self) -> bool {
        self.request_cancel();
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(result) => result.is_ok(),
            Err(_) => {
                task.abort();
                let _ = task.await;
                false
            }
        }
    }
}

pub async fn entered(provider: &Provider) {
    tokio::time::timeout(Duration::from_secs(3), provider.candidate_sources_entered())
        .await
        .expect("the admitted public caller must reach the owned provider gate");
}

pub async fn held(provider: &Provider, restarted: bool) {
    let mut expected = vec![Event::Search, Event::DetailA, Event::SourcesA];
    if restarted {
        expected.extend([Event::Search, Event::DetailA, Event::SourcesA]);
    }
    assert_eq!(
        provider.events().await,
        expected,
        "reserved and Busy callers must not start provider IO"
    );
}

pub async fn settled(provider: &Provider, restarted: bool) {
    let mut expected = Vec::new();
    if restarted {
        expected.extend([Event::Search, Event::DetailA, Event::SourcesA]);
    }
    expected.extend([
        Event::Search,
        Event::DetailA,
        Event::SourcesA,
        Event::DetailB,
        Event::SourcesB,
    ]);
    assert_eq!(
        provider.events().await,
        expected,
        "settled receipts must preserve the exact observed provider exchange sequence"
    );
    assert!(
        provider.authenticated(),
        "owned protocol requests must retain their approved credential"
    );
}

pub fn ready(result: &SourceResolution, account: &Account, request_id: Uuid) {
    assert_eq!(result.request_id, request_id);
    assert!(result.recording_id == account.recording);
    assert!(matches!(result.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(result.sources.len(), 1);
    assert!(result.sources[0].ready && result.sources[0].recording_id == account.recording);
}

pub fn cancelled(result: &SourceResolution, account: &Account, request_id: Uuid) {
    assert_eq!(result.request_id, request_id);
    assert!(result.recording_id == account.recording);
    assert!(matches!(result.outcome, SourceResolutionOutcome::Cancelled));
}

pub async fn replay(hito: &Services, account: &Account, request_id: Uuid) -> SourceResolution {
    tokio::time::timeout(
        Duration::from_secs(1),
        hito.catalog.resolve_source_until(
            &account.actor,
            &account.recording,
            ResolveSource { request_id },
            std::future::pending(),
        ),
    )
    .await
    .expect("the explicit request must settle without waiting for a stale caller")
    .expect("the explicit request must return its own durable receipt")
}

pub async fn no_download(hito: &Services, account: &Account) {
    assert!(
        matches!(
            hito.downloads
                .status(&account.actor, &account.recording)
                .await,
            Err(Error::NotFound)
        ),
        "handoff, cancellation and Busy must not create a download intent"
    );
}

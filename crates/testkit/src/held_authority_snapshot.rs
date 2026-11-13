use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
use tracing::Subscriber;
use tracing_subscriber::{Layer, layer::Context as TraceContext, registry::LookupSpan};

#[derive(Clone)]
pub struct HeldSnapshot(Arc<SnapshotState>);

#[derive(Default)]
struct SnapshotState {
    point: &'static str,
    ancestor_span: Option<&'static str>,
    entered: Notify,
    used: AtomicBool,
    waiting: AtomicBool,
    returned: AtomicBool,
    current: AtomicBool,
    transaction_open: AtomicBool,
    released: Mutex<bool>,
    changed: Condvar,
}

struct Fields {
    selected: &'static str,
    point: bool,
    current: bool,
    transaction_open: bool,
}
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "point" {
            self.point = value == self.selected;
        }
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        match field.name() {
            "snapshot_current" => self.current = value,
            "transaction_open" => self.transaction_open = value,
            _ => {}
        }
    }
}
impl<S> Layer<S> for HeldSnapshot
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, context: TraceContext<'_, S>) {
        if event.metadata().target() != "hito_core::authority_snapshot_test" {
            return;
        }
        if let Some(ancestor) = self.0.ancestor_span {
            let selected = context
                .event_scope(event)
                .is_some_and(|scope| scope.from_root().any(|span| span.name() == ancestor));
            if !selected {
                return;
            }
        }
        let mut fields = Fields {
            selected: self.0.point,
            point: false,
            current: false,
            transaction_open: false,
        };
        event.record(&mut fields);
        if !fields.point || self.0.used.swap(true, Ordering::SeqCst) {
            return;
        }
        self.0.current.store(fields.current, Ordering::SeqCst);
        self.0
            .transaction_open
            .store(fields.transaction_open, Ordering::SeqCst);
        tokio::task::block_in_place(|| {
            self.0.waiting.store(true, Ordering::SeqCst);
            self.0.entered.notify_one();
            let released = self.0.released.lock().unwrap();
            let (released, timed) = self
                .0
                .changed
                .wait_timeout_while(released, Duration::from_secs(4), |released| !*released)
                .unwrap();
            let explicitly_released = *released;
            drop(released);
            self.0.waiting.store(false, Ordering::SeqCst);
            assert!(
                explicitly_released && !timed.timed_out(),
                "the owned SQLite snapshot barrier must be explicitly released"
            );
            self.0.returned.store(true, Ordering::SeqCst);
        });
    }
}
impl HeldSnapshot {
    pub fn at(point: &'static str) -> Self {
        Self(Arc::new(SnapshotState {
            point,
            ..SnapshotState::default()
        }))
    }
    pub fn within_span(point: &'static str, ancestor_span: &'static str) -> Self {
        Self(Arc::new(SnapshotState {
            point,
            ancestor_span: Some(ancestor_span),
            ..SnapshotState::default()
        }))
    }
    pub async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.0.entered.notified())
            .await
            .unwrap();
        assert!(self.0.current.load(Ordering::SeqCst));
        assert!(self.0.transaction_open.load(Ordering::SeqCst));
        self.assert_held();
    }
    pub fn assert_held(&self) {
        assert!(self.0.waiting.load(Ordering::SeqCst));
    }
    pub fn assert_returned(&self) {
        assert!(self.0.returned.load(Ordering::SeqCst));
    }
    pub fn release(&self) {
        *self.0.released.lock().unwrap() = true;
        self.0.changed.notify_all();
    }
}

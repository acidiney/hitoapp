use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
use tracing::Subscriber;
use tracing_subscriber::{Layer, layer::Context};

#[derive(Default)]
struct State {
    claimed: AtomicBool,
    expired: AtomicBool,
    released: Mutex<bool>,
    condition: Condvar,
    entered: Notify,
}

#[derive(Clone, Default)]
pub struct Publication(Arc<State>);

impl Publication {
    pub async fn entered(&self) {
        self.0.entered.notified().await;
    }

    pub fn release(&self) {
        *self
            .0
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.0.condition.notify_all();
    }

    pub fn expired(&self) -> bool {
        self.0.expired.load(Ordering::Acquire)
    }
}

impl<S: Subscriber> Layer<S> for Publication {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        struct Marker(bool);
        impl tracing::field::Visit for Marker {
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "event" && value == "source_resolution_write_started" {
                    self.0 = true;
                }
            }
        }
        let mut marker = Marker(false);
        event.record(&mut marker);
        if !marker.0 || self.0.claimed.swap(true, Ordering::AcqRel) {
            return;
        }
        tokio::task::block_in_place(|| {
            let released = self
                .0
                .released
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.0.entered.notify_one();
            let (released, _) = self
                .0
                .condition
                .wait_timeout_while(released, Duration::from_secs(3), |released| !*released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !*released {
                self.0.expired.store(true, Ordering::Release);
            }
        });
    }
}

use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use tokio::sync::Notify;
use tracing::Subscriber;
use tracing_subscriber::{Layer, layer::Context};

#[derive(Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Armed,
    Held,
    Released,
    Returned,
}

#[derive(Default)]
struct State {
    phase: Mutex<Phase>,
    entered: Notify,
    changed: Condvar,
}

#[derive(Clone, Default)]
pub(crate) struct DispatchPause(Arc<State>);

impl<S: Subscriber> Layer<S> for DispatchPause {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() != "hito_core::catalog_dispatch_test" {
            return;
        }
        struct Point(bool);
        impl tracing::field::Visit for Point {
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "point" && value == "artist_search_before_preflight" {
                    self.0 = true;
                }
            }
        }
        let mut point = Point(false);
        event.record(&mut point);
        if !point.0 {
            return;
        }
        let mut phase = self.0.phase.lock().unwrap();
        if *phase != Phase::Armed {
            return;
        }
        *phase = Phase::Held;
        drop(phase);
        tokio::task::block_in_place(|| {
            self.0.entered.notify_one();
            let phase = self.0.phase.lock().unwrap();
            let (mut phase, timed) = self
                .0
                .changed
                .wait_timeout_while(phase, Duration::from_secs(4), |phase| *phase == Phase::Held)
                .unwrap();
            assert!(
                !timed.timed_out() && *phase == Phase::Released,
                "The owned search preflight pause must be explicitly released"
            );
            *phase = Phase::Returned;
        });
    }
}

impl DispatchPause {
    pub(crate) async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.0.entered.notified())
            .await
            .unwrap();
        self.assert_held();
    }

    pub(crate) fn assert_held(&self) {
        assert_eq!(*self.0.phase.lock().unwrap(), Phase::Held);
    }

    pub(crate) fn release(&self) {
        let mut phase = self.0.phase.lock().unwrap();
        if matches!(*phase, Phase::Armed | Phase::Held) {
            *phase = Phase::Released;
            self.0.changed.notify_all();
        }
    }

    pub(crate) fn assert_returned(&self) {
        assert_eq!(*self.0.phase.lock().unwrap(), Phase::Returned);
    }
}

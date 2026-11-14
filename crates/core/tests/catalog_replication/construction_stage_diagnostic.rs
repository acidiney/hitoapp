use serde::Serialize;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
use tracing::{Metadata, Subscriber};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

const TARGET: &str = "hito_core::catalog_snapshot_stage_test";
const MAX_EVENTS: usize = 32;

#[derive(Default, Serialize)]
struct Observation {
    stage: String,
    elapsed_us: u128,
    catalog_recordings: Option<u64>,
    encoded_pages: Option<u64>,
    encoded_bytes: Option<u64>,
}

impl tracing::field::Visit for Observation {
    fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "stage" && value.len() <= 32 {
            self.stage = value.to_owned();
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        match field.name() {
            "catalog_recordings" => self.catalog_recordings = Some(value),
            "encoded_pages" => self.encoded_pages = Some(value),
            "encoded_bytes" => self.encoded_bytes = Some(value),
            _ => {}
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if let Ok(value) = u64::try_from(value) {
            self.record_u64(field, value);
        }
    }
}

struct State {
    started: Instant,
    events: Vec<Observation>,
    overflowed: bool,
}

#[derive(Clone)]
pub(super) struct ConstructionStages(Arc<Mutex<State>>);

impl<S: Subscriber> Layer<S> for ConstructionStages {
    fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
        metadata.target() == TARGET
    }

    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() != TARGET {
            return;
        }
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.events.len() == MAX_EVENTS {
            state.overflowed = true;
            return;
        }
        let mut observation = Observation {
            elapsed_us: state.started.elapsed().as_micros(),
            ..Observation::default()
        };
        event.record(&mut observation);
        state.events.push(observation);
    }
}

impl ConstructionStages {
    pub(super) fn new(started: Instant) -> Self {
        Self(Arc::new(Mutex::new(State {
            started,
            events: Vec::new(),
            overflowed: false,
        })))
    }

    pub(super) fn dispatch(&self) -> tracing::Dispatch {
        hito_testkit::logs::anchored_dispatch(tracing_subscriber::registry().with(self.clone()))
    }

    pub(super) fn report(&self) -> Value {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        json!({"overflowed": state.overflowed, "events": state.events})
    }
}

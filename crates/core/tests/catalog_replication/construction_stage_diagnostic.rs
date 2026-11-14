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
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome_code: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_phase: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_partial_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_batches: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_recordings: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fetched_source_rows: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assembled_source_facts: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_pages: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_preflight_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_preflight_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording_preflight_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording_preflight_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording_batch_query_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording_batch_query_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_preflight_query_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_preflight_query_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_facts_query_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_facts_query_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_fact_assembly_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_fact_assembly_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording_fact_assembly_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording_fact_assembly_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_page_encoding_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_page_encoding_calls: Option<u64>,
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
            "outcome_code" => self.outcome_code = Some(value),
            "active_phase" => self.active_phase = Some(value),
            "active_partial_us" => self.active_partial_us = Some(value),
            "completed_batches" => self.completed_batches = Some(value),
            "completed_recordings" => self.completed_recordings = Some(value),
            "fetched_source_rows" => self.fetched_source_rows = Some(value),
            "assembled_source_facts" => self.assembled_source_facts = Some(value),
            "completed_pages" => self.completed_pages = Some(value),
            "catalog_preflight_us" => self.catalog_preflight_us = Some(value),
            "catalog_preflight_calls" => self.catalog_preflight_calls = Some(value),
            "recording_preflight_us" => self.recording_preflight_us = Some(value),
            "recording_preflight_calls" => self.recording_preflight_calls = Some(value),
            "recording_batch_query_us" => self.recording_batch_query_us = Some(value),
            "recording_batch_query_calls" => self.recording_batch_query_calls = Some(value),
            "source_preflight_query_us" => self.source_preflight_query_us = Some(value),
            "source_preflight_query_calls" => self.source_preflight_query_calls = Some(value),
            "source_facts_query_us" => self.source_facts_query_us = Some(value),
            "source_facts_query_calls" => self.source_facts_query_calls = Some(value),
            "source_fact_assembly_us" => self.source_fact_assembly_us = Some(value),
            "source_fact_assembly_calls" => self.source_fact_assembly_calls = Some(value),
            "recording_fact_assembly_us" => self.recording_fact_assembly_us = Some(value),
            "recording_fact_assembly_calls" => self.recording_fact_assembly_calls = Some(value),
            "final_page_encoding_us" => self.final_page_encoding_us = Some(value),
            "final_page_encoding_calls" => self.final_page_encoding_calls = Some(value),
            _ => {}
        }
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == "completed" {
            self.completed = Some(value);
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

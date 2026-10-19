use std::{
    fmt,
    sync::{Arc, Mutex},
};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    level_filters::LevelFilter,
    span::{Attributes, Id},
    subscriber::Interest,
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

#[derive(Clone, Default)]
pub struct Audit(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    created: [usize; 3],
    entered: [usize; 3],
    exited: [usize; 3],
    events: Vec<Observation>,
    overflow: bool,
}

struct Observation {
    workflow: &'static str,
    outcome: &'static str,
    kinds: [usize; 3],
    valid: [bool; 3],
}

struct Marker {
    kind: usize,
    valid: bool,
}

#[derive(Default)]
struct Values {
    valid: bool,
    workflow: Option<&'static str>,
    outcome: Option<&'static str>,
}

impl Audit {
    pub fn report(&self) -> serde_json::Value {
        let state = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(
            !state.overflow,
            "The scoped logging audit must stay bounded"
        );
        serde_json::json!({
            "kind_order": ["request", "http", "core"],
            "created": state.created, "entered": state.entered, "exited": state.exited,
            "events": state.events.iter().map(|event| serde_json::json!({
                "workflow": event.workflow, "outcome": event.outcome,
                "scope_kinds": event.kinds, "uuid_valid": event.valid,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn assert_observed(&self) {
        let state = self.0.lock().unwrap();
        assert!(!state.overflow);
        assert_eq!(state.created, [3, 3, 3]);
        assert!(state.entered.iter().all(|count| *count > 0));
        assert_eq!(state.entered, state.exited);
        assert_eq!(state.events.len(), 6);
        for workflow in ["core", "http"] {
            for outcome in ["conflict", "unauthorized", "ready"] {
                assert_eq!(
                    state
                        .events
                        .iter()
                        .filter(|event| event.workflow == workflow && event.outcome == outcome)
                        .count(),
                    1
                );
            }
        }
        for event in &state.events {
            let expected = if event.workflow == "core" {
                [1, 1, 1]
            } else {
                [1, 1, 0]
            };
            assert_eq!(event.kinds, expected);
            assert_eq!(event.valid, expected.map(|count| count == 1));
        }
    }
}

impl<S> Layer<S> for Audit
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::always()
    }
    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::OFF)
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(kind) = kind(attrs.metadata()) else {
            return;
        };
        let mut values = Values::default();
        attrs.record(&mut values);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Marker {
                kind,
                valid: values.valid,
            });
        }
        self.0.lock().unwrap().created[kind] += 1;
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && let Some(marker) = span.extensions().get::<Marker>()
        {
            self.0.lock().unwrap().entered[marker.kind] += 1;
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && let Some(marker) = span.extensions().get::<Marker>()
        {
            self.0.lock().unwrap().exited[marker.kind] += 1;
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if !matches!(
            event.metadata().target(),
            "hito_core::catalog::timed_content" | "hito_server::api::lyrics"
        ) {
            return;
        }
        let mut values = Values::default();
        event.record(&mut values);
        let Some(workflow) = values.workflow else {
            return;
        };
        let mut kinds = [0; 3];
        let mut valid = [false; 3];
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope {
                if let Some(marker) = span.extensions().get::<Marker>() {
                    kinds[marker.kind] += 1;
                    valid[marker.kind] |= marker.valid;
                }
            }
        }
        let mut state = self.0.lock().unwrap();
        if state.events.len() == 32 {
            state.overflow = true;
            return;
        }
        state.events.push(Observation {
            workflow,
            outcome: values.outcome.unwrap_or("other"),
            kinds,
            valid,
        });
    }
}

fn kind(metadata: &Metadata<'_>) -> Option<usize> {
    match (metadata.target(), metadata.name()) {
        ("hito_server::telemetry", "http_request") => Some(0),
        ("hito_server::api::lyrics", "read") => Some(1),
        ("hito_core::catalog::timed_content", "recording_lyrics_until") => Some(2),
        _ => None,
    }
}

impl Values {
    fn record(&mut self, field: &Field, value: &str) {
        match field.name() {
            "request_id" => {
                self.valid = uuid::Uuid::parse_str(value).is_ok_and(|id| id.get_version_num() == 4)
            }
            "message" => {
                self.workflow = match value {
                    "Recording lyric lookup completed" => Some("core"),
                    "Recording lyric HTTP request completed" => Some("http"),
                    _ => None,
                }
            }
            "outcome" => {
                self.outcome = Some(match value {
                    "conflict" => "conflict",
                    "unauthorized" => "unauthorized",
                    "ready" => "ready",
                    "cancelled" => "cancelled",
                    "not_found" => "not_found",
                    "unavailable" => "unavailable",
                    "unsupported" => "unsupported",
                    _ => "other",
                })
            }
            _ => {}
        }
    }
}

impl Visit for Values {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.record(field, value);
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if matches!(field.name(), "request_id" | "message" | "outcome") {
            self.record(field, &format!("{value:?}"));
        }
    }
}

use std::{
    fmt::{self, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

#[derive(Clone)]
pub struct Capture {
    text: Arc<Mutex<String>>,
    sequence: Arc<AtomicU64>,
    _ambient: tracing::Dispatch,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            text: Arc::default(),
            sequence: Arc::default(),
            _ambient: tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()),
        }
    }
}

impl Capture {
    pub fn text(&self) -> String {
        self.text
            .lock()
            .expect("fixture log capture failed")
            .clone()
    }

    fn append(&self, fields: Fields) {
        let mut text = self.text.lock().expect("fixture log capture failed");
        assert!(
            text.len() + fields.0.len() < 1_048_576,
            "fixture logs exceeded their bound"
        );
        text.push_str(&fields.0);
        text.push('\n');
    }
}

impl Subscriber for Capture {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target() == "hito_plugin_jamendo"
            || metadata.target().starts_with("hito_plugin_jamendo::")
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        self.append(fields);
        Id::from_u64(self.sequence.fetch_add(1, Ordering::Relaxed) + 1)
    }

    fn record(&self, _span: &Id, values: &Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        self.append(fields);
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.append(fields);
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[derive(Default)]
struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        write!(&mut self.0, "{}={value:?} ", field.name()).expect("fixture log fields failed");
    }
}

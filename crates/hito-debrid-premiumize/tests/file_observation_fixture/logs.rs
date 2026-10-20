use serde_json::Value;
use std::{
    collections::BTreeSet,
    io::{self, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct Capture {
    bytes: Arc<Mutex<Vec<u8>>>,
    overflowed: Arc<AtomicBool>,
}

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        let capture = self.clone();
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter("off,hito_debrid_premiumize=trace,hito_remote_audio=trace")
                .without_time()
                .with_ansi(false)
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(move || capture.clone())
                .finish(),
        )
    }

    pub fn assert_available(&self, private: &[&str]) {
        self.assert_outcomes(&["provider_completed", "provider_seeding"], private);
    }

    pub fn assert_empty(&self) {
        assert!(!self.overflowed.load(Ordering::SeqCst));
        assert!(
            self.bytes.lock().unwrap().is_empty(),
            "the unrelated dispatcher received workflow logs"
        );
    }

    pub fn assert_outcomes(&self, expected: &[&str], private: &[&str]) {
        assert!(
            !self.overflowed.load(Ordering::SeqCst),
            "workflow log capture exceeded its bound"
        );
        let bytes = self.bytes.lock().unwrap();
        let text = std::str::from_utf8(&bytes).expect("workflow logs must be UTF-8");
        for forbidden in private
            .iter()
            .copied()
            .chain(["http://", "https://", "http%3A", "https%3A", "Bearer "])
        {
            assert!(!forbidden.is_empty());
            let escaped = serde_json::to_string(forbidden).unwrap();
            assert!(
                !text.contains(forbidden) && !text.contains(&escaped[1..escaped.len() - 1]),
                "workflow logs exposed private fixture data"
            );
        }
        let events: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("workflow logs must be structured JSON"))
            .collect();
        assert_eq!(events.len(), expected.len());
        let mut correlations = BTreeSet::new();
        for (event, outcome) in events.iter().zip(expected) {
            assert_keys(event, &["level", "fields", "target", "span", "spans"]);
            assert_eq!(event["level"], "DEBUG");
            assert_eq!(event["target"], "hito_debrid_premiumize::file_observation");
            assert_keys(
                &event["fields"],
                &["event", "outcome", "count", "elapsed_ms"],
            );
            assert_eq!(
                event["fields"]["event"],
                "premiumize_file_observation_finished"
            );
            assert_eq!(event["fields"]["outcome"], *outcome);
            assert_eq!(
                event["fields"]["count"].as_u64(),
                Some(u64::from(matches!(
                    *outcome,
                    "provider_completed" | "provider_seeding"
                )))
            );
            assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
            let spans = event["spans"].as_array().unwrap();
            assert_eq!(spans.len(), 1);
            assert_eq!(event["span"], spans[0]);
            assert_keys(&spans[0], &["name", "request_id", "operation"]);
            assert_eq!(spans[0]["name"], "premiumize_file_observation");
            assert_eq!(spans[0]["operation"], "bound_file_observation");
            let id = Uuid::parse_str(spans[0]["request_id"].as_str().unwrap()).unwrap();
            assert_eq!(id.get_version_num(), 4);
            assert!(correlations.insert(id));
        }
    }
}

fn assert_keys(value: &Value, expected: &[&str]) {
    let keys: BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, expected.iter().copied().collect());
}

impl Write for Capture {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let mut bytes = self.bytes.lock().unwrap();
        if input.len() > 65_536_usize.saturating_sub(bytes.len()) {
            self.overflowed.store(true, Ordering::SeqCst);
            return Err(io::Error::other("workflow log capture exceeded its bound"));
        }
        bytes.extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

use std::{
    io::Write,
    sync::{Arc, Mutex},
};

const MAX_BYTES: usize = 262_144;

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    overflow: bool,
}

#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Captured>>);

impl Logs {
    pub fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync + 'static {
        let writer = self.clone();
        tracing_subscriber::fmt()
            .with_env_filter("off,hito_core=trace,hito_server=trace")
            .json()
            .with_ansi(false)
            .without_time()
            .with_writer(move || writer.clone())
            .finish()
    }

    pub fn assert_private(&self, canaries: &[&str], outcomes: &[&str]) {
        let captured = self.0.lock().unwrap();
        assert!(
            !captured.overflow,
            "The scoped lyric diagnostic capture must stay bounded"
        );
        let text = std::str::from_utf8(&captured.bytes).unwrap();
        for canary in canaries {
            assert!(!canary.is_empty());
            let encoded = serde_json::to_string(canary).unwrap();
            let escaped = &encoded[1..encoded.len() - 1];
            assert!(
                !text.contains(canary) && !text.contains(escaped),
                "Scoped lyric workflow logs must exclude every private canary"
            );
        }
        let events: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let unique = outcomes
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        for outcome in unique {
            let expected = outcomes
                .iter()
                .filter(|candidate| **candidate == outcome)
                .count();
            for message in [
                "Recording lyric lookup completed",
                "Recording lyric HTTP request completed",
            ] {
                assert_events(&events, message, outcome, expected);
            }
        }
    }

    pub fn assert_core_outcome(&self, outcome: &str) {
        let captured = self.0.lock().unwrap();
        assert!(!captured.overflow);
        let text = std::str::from_utf8(&captured.bytes).unwrap();
        let events: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_events(&events, "Recording lyric lookup completed", outcome, 1);
    }
}

fn assert_events(events: &[serde_json::Value], message: &str, outcome: &str, expected: usize) {
    let matching = events
        .iter()
        .filter(|event| {
            event["fields"]["message"] == message && event["fields"]["outcome"] == outcome
        })
        .collect::<Vec<_>>();
    assert!(
        matching.len() == expected,
        "Each lyric workflow must record its safe outcome: workflow={} expected_outcome={} expected_count={} actual_count={} total={}",
        if message == "Recording lyric lookup completed" {
            "core"
        } else {
            "http"
        },
        outcome,
        expected,
        matching.len(),
        events.len()
    );
    for event in matching {
        assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
        assert!(
            event["spans"]
                .as_array()
                .unwrap()
                .iter()
                .any(|span| span["request_id"]
                    .as_str()
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())),
            "Each lyric outcome must retain a safe request correlation"
        );
    }
}

impl Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut captured = self.0.lock().unwrap();
        let accepted = bytes
            .len()
            .min(MAX_BYTES.saturating_sub(captured.bytes.len()));
        captured.bytes.extend_from_slice(&bytes[..accepted]);
        captured.overflow |= accepted != bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

use std::{
    io::Write,
    sync::{Arc, Mutex},
};

const MAX_BYTES: usize = 262_144;
const SETTLE_YIELDS: usize = 256;

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    overflow: bool,
}

#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Captured>>);

impl Logs {
    pub fn subscriber(
        &self,
    ) -> impl tracing::Subscriber
    + for<'a> tracing_subscriber::registry::LookupSpan<'a>
    + Send
    + Sync
    + 'static {
        hito_testkit::logs::anchor_callsite_interest();
        let writer = self.clone();
        tracing_subscriber::fmt()
            .with_env_filter("off,hito_core=trace,hito_server=trace")
            .json()
            .with_ansi(false)
            .without_time()
            .with_writer(move || writer.clone())
            .finish()
    }

    pub fn subscriber_with_close_events(&self) -> impl tracing::Subscriber + Send + Sync + 'static {
        hito_testkit::logs::anchor_callsite_interest();
        let writer = self.clone();
        tracing_subscriber::fmt()
            .with_env_filter("off,hito_core=trace,hito_server=trace")
            .json()
            .with_ansi(false)
            .without_time()
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .with_writer(move || writer.clone())
            .finish()
    }

    pub fn assert_same_capture(&self, other: &Self, expected_events: usize) {
        let captured = self.0.lock().unwrap();
        let other = other.0.lock().unwrap();
        assert!(!captured.overflow && !other.overflow);
        assert_eq!(
            std::str::from_utf8(&captured.bytes)
                .unwrap()
                .lines()
                .count(),
            expected_events
        );
        assert!(
            captured.bytes == other.bytes,
            "The audit must preserve exact JSON output"
        );
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

    pub fn assert_empty(&self) {
        let captured = self.0.lock().unwrap();
        assert!(!captured.overflow);
        assert!(
            captured.bytes.is_empty(),
            "No workflow may enter this capture"
        );
    }

    pub async fn assert_span_closed(&self, name: &str) {
        let closed = || {
            let captured = self.0.lock().unwrap();
            assert!(!captured.overflow);
            let text = std::str::from_utf8(&captured.bytes).unwrap();
            text.lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .filter(|event| {
                    event["fields"]["message"] == "close" && event["span"]["name"] == name
                })
                .count()
        };
        let first = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let count = closed();
                if count > 0 {
                    return count;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap_or(0);
        for _ in 0..SETTLE_YIELDS {
            tokio::task::yield_now().await;
        }
        let count = if first == 0 { 0 } else { closed() };
        assert_eq!(
            count, 1,
            "The owner must observe exactly one span retirement: span={name}"
        );
    }

    pub fn assert_core_outcomes(&self, outcomes: &[&str]) {
        let captured = self.0.lock().unwrap();
        assert!(!captured.overflow);
        let text = std::str::from_utf8(&captured.bytes).unwrap();
        let events: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let completions = events
            .iter()
            .filter(|event| event["fields"]["message"] == "Recording lyric lookup completed")
            .collect::<Vec<_>>();
        assert_eq!(completions.len(), outcomes.len());
        let mut correlations = std::collections::BTreeSet::new();
        for event in completions {
            let fields = event["fields"].as_object().unwrap();
            assert_eq!(fields.len(), 3);
            assert!(fields["elapsed_ms"].as_u64().is_some());
            let span = event["spans"]
                .as_array()
                .unwrap()
                .iter()
                .find(|span| span["operation"] == "recording_lyrics")
                .expect("Each completion must retain its original lyric span");
            let correlation = uuid::Uuid::parse_str(span["request_id"].as_str().unwrap()).unwrap();
            assert_eq!(correlation.get_version_num(), 4);
            assert!(correlations.insert(correlation));
        }
        for outcome in outcomes
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
        {
            assert_events(
                &events,
                "Recording lyric lookup completed",
                outcome,
                outcomes
                    .iter()
                    .filter(|candidate| **candidate == outcome)
                    .count(),
            );
        }
    }

    pub fn assert_provider_lists(&self, outcomes: &[&str]) {
        let captured = self.0.lock().unwrap();
        assert!(!captured.overflow);
        let text = std::str::from_utf8(&captured.bytes).unwrap();
        let events: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|event: &serde_json::Value| {
                event["fields"]["message"] == "Recording lyric provider list completed"
            })
            .collect();
        assert_eq!(events.len(), outcomes.len());
        let mut correlations = std::collections::BTreeSet::new();
        for (event, outcome) in events.iter().zip(outcomes) {
            let fields = event["fields"].as_object().unwrap();
            assert_eq!(fields.len(), 3);
            assert!(fields["outcome"] == *outcome && fields["elapsed_ms"].as_u64().is_some());
            let owners: Vec<_> = event["spans"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|span| span["operation"] == "recording_lyric_providers")
                .collect();
            assert_eq!(owners.len(), 1);
            let correlation =
                uuid::Uuid::parse_str(owners[0]["request_id"].as_str().unwrap()).unwrap();
            assert_eq!(correlation.get_version_num(), 4);
            assert!(correlations.insert(correlation));
        }
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
        let spans = event["spans"].as_array();
        assert!(
            spans.is_some_and(|spans| spans.iter().any(|span| span["request_id"]
                .as_str()
                .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()))),
            "Each lyric outcome must retain a safe request correlation: workflow={} outcome={} span_count={} singular_span_present={}",
            if message == "Recording lyric lookup completed" {
                "core"
            } else {
                "http"
            },
            outcome,
            spans.map_or(0, Vec::len),
            event["span"].is_object(),
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

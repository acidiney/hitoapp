use hito_testkit::logs::LogCapture;
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

pub enum Expected {
    Opening(&'static str),
    Body(&'static str, u64),
}

#[derive(Clone, Default)]
pub struct Capture(LogCapture);

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        self.0
            .dispatch_with_spans("off,hito_debrid_premiumize=trace,hito_remote_audio=trace")
    }

    pub fn assert_complete(&self, bytes_delivered: u64, private: &[&str]) {
        self.assert_events(
            &[
                Expected::Opening("complete"),
                Expected::Body("complete", bytes_delivered),
            ],
            private,
        );
    }

    pub fn assert_empty(&self) {
        assert!(
            self.0.text().is_empty(),
            "the unrelated dispatcher received workflow logs"
        );
    }

    pub fn assert_events(&self, expected: &[Expected], private: &[&str]) {
        let text = self.0.text();
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
        let mut openings = BTreeSet::new();
        let mut complete_openings = BTreeSet::new();
        let mut bodies = BTreeSet::new();
        for (event, expected) in events.iter().zip(expected) {
            assert_keys(event, &["level", "fields", "target", "span", "spans"]);
            assert_eq!(event["level"], "DEBUG");
            assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
            let spans = event["spans"]
                .as_array()
                .expect("workflow spans must exist");
            assert_eq!(spans.len(), 1);
            assert_eq!(event["span"], spans[0]);
            assert_keys(&spans[0], &["name", "request_id", "operation"]);
            assert_eq!(spans[0]["name"], "premiumize_audio");
            assert_eq!(spans[0]["operation"], "bound_audio_opening");
            let id = Uuid::parse_str(spans[0]["request_id"].as_str().unwrap()).unwrap();
            assert_eq!(id.get_version_num(), 4);
            match expected {
                Expected::Opening(outcome) => {
                    assert_eq!(event["target"], "hito_debrid_premiumize::audio");
                    assert_keys(
                        &event["fields"],
                        &["event", "outcome", "count", "elapsed_ms"],
                    );
                    assert_eq!(
                        event["fields"]["event"],
                        "premiumize_audio_opening_finished"
                    );
                    assert_eq!(event["fields"]["outcome"], *outcome);
                    assert_eq!(
                        event["fields"]["count"].as_u64(),
                        Some(u64::from(*outcome == "complete"))
                    );
                    assert!(openings.insert(id), "an opening completed more than once");
                    if *outcome == "complete" {
                        complete_openings.insert(id);
                    }
                }
                Expected::Body(outcome, delivered) => {
                    assert_eq!(event["target"], "hito_remote_audio::body");
                    assert_keys(
                        &event["fields"],
                        &["message", "event", "outcome", "bytes", "elapsed_ms"],
                    );
                    assert_eq!(event["fields"]["event"], "remote_audio_body_finished");
                    assert_eq!(event["fields"]["outcome"], *outcome);
                    assert_eq!(event["fields"]["bytes"].as_u64(), Some(*delivered));
                    assert_eq!(event["fields"]["message"], "Remote audio body finished");
                    assert!(
                        complete_openings.contains(&id),
                        "a body lost its opening correlation"
                    );
                    assert!(bodies.insert(id), "a body completed more than once");
                }
            }
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

use hito_testkit::logs::LogCapture;
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct Capture(LogCapture);

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        self.0
            .dispatch_with_spans("off,hito_debrid_all_debrid=trace,hito_remote_audio=trace")
    }

    pub fn is_empty(&self) -> bool {
        self.0.text().is_empty()
    }

    pub fn text(&self) -> String {
        let text = self.0.text();
        assert!(!text.is_empty(), "Workflow logs must not be empty");
        text
    }

    pub fn assert_workflows(&self, expected: &[(&str, &str, u64)], private: &[&str]) {
        let text = self.text();
        for forbidden in private
            .iter()
            .copied()
            .chain(["http://", "https://", "http%3A", "https%3A", "Bearer "])
        {
            assert!(!forbidden.is_empty());
            let escaped = serde_json::to_string(forbidden).unwrap();
            let escaped = &escaped[1..escaped.len() - 1];
            assert!(
                !text.contains(forbidden) && !text.contains(escaped),
                "Workflow logs exposed private fixture data"
            );
        }
        let events: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("Workflow logs must be structured JSON"))
            .collect();
        assert!(!expected.is_empty());
        assert_eq!(events.len(), expected.len());
        let mut correlations = BTreeSet::new();
        for (event, (operation, outcome, count)) in events.iter().zip(expected) {
            assert_keys(event, &["level", "fields", "target", "span", "spans"]);
            assert_eq!(event["level"], "DEBUG");
            assert_eq!(event["target"], "hito_debrid_all_debrid::client");
            assert_keys(
                &event["fields"],
                &["event", "outcome", "count", "elapsed_ms"],
            );
            let (name, completed) = match *operation {
                "magnet_observation" => {
                    ("all_debrid_observation", "all_debrid_observation_finished")
                }
                "magnet_submission" => ("all_debrid_submission", "all_debrid_submission_finished"),
                "bound_file_preparation" => {
                    ("all_debrid_preparation", "all_debrid_preparation_finished")
                }
                "delayed_file_poll" => (
                    "all_debrid_delayed_poll",
                    "all_debrid_delayed_poll_finished",
                ),
                _ => panic!("The fixture must name a known workflow"),
            };
            assert_eq!(event["fields"]["event"], completed);
            assert_eq!(event["fields"]["outcome"], *outcome);
            assert_eq!(event["fields"]["count"].as_u64(), Some(*count));
            assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
            let spans = event["spans"].as_array().unwrap();
            assert_eq!(spans.len(), 1);
            assert_eq!(event["span"], spans[0]);
            let span = &spans[0];
            assert_keys(span, &["name", "request_id", "operation"]);
            assert_eq!(span["name"], name);
            assert_eq!(span["operation"], *operation);
            let correlation = Uuid::parse_str(span["request_id"].as_str().unwrap()).unwrap();
            assert_eq!(correlation.get_version_num(), 4);
            assert!(correlations.insert(correlation));
        }
    }
}

fn assert_keys(value: &Value, expected: &[&str]) {
    let keys = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(keys, expected.iter().copied().collect());
}

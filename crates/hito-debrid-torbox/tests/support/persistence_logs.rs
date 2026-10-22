use hito_testkit::logs::LogCapture;
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct Capture(LogCapture);

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        self.0.dispatch_with_spans("off,hito_debrid_torbox=debug")
    }

    pub fn assert_events(&self, expected: &[(&str, &str, u64)], private_ids: &[Uuid]) -> usize {
        let text = self.0.text();
        let events: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("persistence events must be JSON"))
            .collect();
        assert_eq!(events.len(), expected.len());
        let mut correlations = BTreeSet::new();
        for (event, (operation, outcome, count)) in events.iter().zip(expected) {
            assert_keys(event, &["level", "fields", "target", "span", "spans"]);
            assert!(event["level"] == "DEBUG");
            assert!(event["target"] == "hito_debrid_torbox::workflow");
            let fields = &event["fields"];
            assert_keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
            let event_name = match *operation {
                "candidate_authority" => "torbox_candidate_authority_finished",
                "candidate_sealing" => "torbox_candidate_sealing_finished",
                "candidate_restoration" => "torbox_candidate_restoration_finished",
                _ => panic!("only persistence workflows belong in this capture"),
            };
            assert!(fields["event"] == event_name);
            assert!(fields["outcome"] == *outcome);
            assert!(fields["count"].as_u64() == Some(*count));
            assert!(fields["elapsed_ms"].as_u64().is_some());
            let spans = event["spans"]
                .as_array()
                .expect("one persistence span must exist");
            assert_eq!(spans.len(), 1);
            let span = &spans[0];
            assert!(event["span"] == *span);
            assert_keys(span, &["name", "request_id", "operation"]);
            assert!(span["name"] == "torbox_candidate_persistence");
            assert!(span["operation"] == *operation);
            let id = span["request_id"]
                .as_str()
                .and_then(|value| Uuid::parse_str(value).ok())
                .expect("persistence correlation must be a UUID");
            assert_eq!(id.get_version_num(), 4);
            assert!(!private_ids.contains(&id));
            assert!(correlations.insert(id));
        }
        events.len()
    }
}

fn assert_keys(value: &Value, expected: &[&str]) {
    let keys = value
        .as_object()
        .expect("persistence schema objects must exist")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert!(
        keys == expected.iter().copied().collect(),
        "persistence logs must contain only the safe schema"
    );
}

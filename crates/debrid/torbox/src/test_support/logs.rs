use hito_testkit::logs::LogCapture;
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct Capture(LogCapture);

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        self.0
            .dispatch_with_spans("off,hito_debrid_torbox=trace,hito_remote_audio=trace")
    }

    pub fn assert_operations(&self, expected: &[(&str, &str)], private: &[&str]) {
        let text = self.0.text();
        for forbidden in private.iter().copied().chain([
            "https://",
            "http://",
            "https%3A",
            "Bearer ",
            "token=",
            "owned-torbox-key%2F%2B%3F%3D%26-kept-private",
            "owned-private-media-signature",
            "owned-private-non-audio-response",
            "owned-private-media-cookie",
            "owned-private-provider-name.wav",
        ]) {
            let escaped = serde_json::to_string(forbidden).unwrap();
            assert!(
                !text.contains(forbidden) && !text.contains(&escaped[1..escaped.len() - 1]),
                "private control data reached workflow logs"
            );
        }
        let events: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.len(), expected.len());
        for (event, (operation, outcome)) in events.iter().zip(expected) {
            assert_keys(event, &["level", "fields", "target", "span", "spans"]);
            let body = event["target"] == "hito_remote_audio::body";
            assert_keys(
                &event["fields"],
                if body {
                    &["event", "message", "outcome", "bytes", "elapsed_ms"]
                } else {
                    &["event", "outcome", "count", "elapsed_ms"]
                },
            );
            assert!(
                event["fields"][if body { "bytes" } else { "count" }]
                    .as_u64()
                    .is_some()
            );
            for span in event["spans"].as_array().unwrap() {
                assert_keys(span, &["name", "request_id", "operation"]);
            }
            assert_eq!(event["level"], "DEBUG");
            assert_eq!(event["span"]["operation"], *operation);
            assert_eq!(event["fields"]["outcome"], *outcome);
            assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
            assert_eq!(
                Uuid::parse_str(event["span"]["request_id"].as_str().unwrap())
                    .unwrap()
                    .get_version_num(),
                4
            );
        }
    }

    pub fn assert_flows(&self, private: &[&str], bytes: u64) {
        let text = self.0.text();
        for forbidden in private.iter().copied().chain([
            "http://",
            "https://",
            "Bearer ",
            "token=",
            "owned-private-media-cookie",
            "owned-private-provider-name.wav",
        ]) {
            assert!(!forbidden.is_empty());
            let escaped = serde_json::to_string(forbidden).unwrap();
            assert!(
                !text.contains(forbidden) && !text.contains(&escaped[1..escaped.len() - 1]),
                "private provider data reached workflow logs"
            );
        }
        let events: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let expected = [
            ("torrent_observation", "provider_completed", 1),
            ("bound_file_preparation", "candidate", 1),
            ("torrent_observation", "provider_completed", 1),
            ("bound_audio_opening", "complete", 1),
            ("bound_audio_opening", "complete", bytes),
            ("torrent_observation", "incomplete", 1),
            ("bound_file_preparation", "incomplete", 0),
            ("torrent_observation", "source_changed", 0),
            ("bound_file_preparation", "source_changed", 0),
        ];
        assert_eq!(events.len(), expected.len());
        let mut correlations = BTreeSet::new();
        let mut opening = None;
        for (index, (event, (operation, outcome, count))) in events.iter().zip(expected).enumerate()
        {
            assert_keys(event, &["level", "fields", "target", "span", "spans"]);
            assert_eq!(event["level"], "DEBUG");
            let body = index == 4;
            if body {
                assert_eq!(event["target"], "hito_remote_audio::body");
            } else {
                assert!(
                    event["target"]
                        .as_str()
                        .unwrap()
                        .starts_with("hito_debrid_torbox::")
                );
            }
            let fields = &event["fields"];
            assert_keys(
                fields,
                if body {
                    &["event", "message", "outcome", "bytes", "elapsed_ms"]
                } else {
                    &["event", "outcome", "count", "elapsed_ms"]
                },
            );
            let expected_event = if body {
                "remote_audio_body_finished"
            } else {
                match operation {
                    "torrent_observation" => "torbox_observation_finished",
                    "bound_file_preparation" => "torbox_preparation_finished",
                    "bound_audio_opening" => "torbox_audio_opening_finished",
                    _ => unreachable!(),
                }
            };
            assert_eq!(fields["event"], expected_event);
            assert_eq!(fields["outcome"], outcome);
            assert_eq!(
                fields[if body { "bytes" } else { "count" }].as_u64(),
                Some(count)
            );
            assert!(fields["elapsed_ms"].as_u64().is_some());
            let spans = event["spans"].as_array().unwrap();
            assert_eq!(
                spans.len(),
                if operation == "torrent_observation" {
                    2
                } else {
                    1
                }
            );
            let current = spans.last().unwrap();
            assert_eq!(event["span"], *current);
            assert_eq!(current["operation"], operation);
            for span in spans {
                assert_keys(span, &["name", "request_id", "operation"]);
            }
            let id = Uuid::parse_str(current["request_id"].as_str().unwrap()).unwrap();
            assert_eq!(id.get_version_num(), 4);
            if body {
                assert_eq!(Some(id), opening);
            } else {
                assert!(correlations.insert(id));
            }
            if index == 3 {
                opening = Some(id);
            }
            if operation == "torrent_observation" {
                let parent_index = if index == 0 {
                    1
                } else if index == 2 {
                    3
                } else {
                    index + 1
                };
                assert_eq!(
                    spans[0]["request_id"],
                    events[parent_index]["span"]["request_id"]
                );
            }
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

use super::{Capture, keys};
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

impl Capture {
    pub fn assert_pre_unlock_retry(&self, private: &[&str]) {
        let text = self.0.text();
        assert!(!text.is_empty());
        for secret in private
            .iter()
            .copied()
            .chain(["http://", "https://", "http%3A", "https%3A", "Bearer "])
        {
            assert!(!secret.is_empty());
            let escaped = serde_json::to_string(secret).unwrap();
            assert!(!text.contains(secret) && !text.contains(&escaped[1..escaped.len() - 1]));
        }
        let events = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 6);
        let mut requests = BTreeSet::new();
        let mut owners = Vec::new();
        let mut provider_ids = BTreeSet::new();
        let mut outcomes = Vec::new();
        for event in &events {
            keys(event, &["level", "fields", "target", "span", "spans"]);
            assert_eq!(event["level"], "DEBUG");
            let spans = event["spans"].as_array().unwrap();
            assert_eq!(&event["span"], spans.last().unwrap());
            for span in spans {
                keys(span, &["name", "operation", "request_id"]);
                assert_eq!(
                    Uuid::parse_str(span["request_id"].as_str().unwrap())
                        .unwrap()
                        .get_version_num(),
                    4
                );
            }
            let owner = &spans[0];
            assert_eq!(owner["name"], "all_debrid_plugin_request");
            assert_eq!(owner["operation"], "prepare");
            let id = Uuid::parse_str(owner["request_id"].as_str().unwrap()).unwrap();
            owners.push(id);
            let fields = &event["fields"];
            assert!(fields["elapsed_ms"].as_u64().is_some());
            match fields["event"].as_str().unwrap() {
                "all_debrid_plugin_request_finished" => {
                    assert_eq!(event["target"], "hito_plugin_all_debrid::http");
                    assert_eq!(spans.len(), 1);
                    keys(fields, &["event", "outcome", "status", "elapsed_ms"]);
                    assert_eq!(fields["outcome"], "success");
                    assert_eq!(fields["status"].as_u64(), Some(200));
                    assert!(requests.insert(id));
                }
                "all_debrid_preparation_finished" => {
                    assert_eq!(event["target"], "hito_debrid_all_debrid::client");
                    assert_eq!(spans.len(), 2);
                    assert_eq!(spans[1]["name"], "all_debrid_preparation");
                    assert_eq!(spans[1]["operation"], "bound_file_preparation");
                    assert_ne!(spans[1]["request_id"], owner["request_id"]);
                    assert!(provider_ids.insert(spans[1]["request_id"].as_str().unwrap()));
                    keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                    outcomes.push((
                        fields["outcome"].as_str().unwrap(),
                        fields["count"].as_u64().unwrap(),
                    ));
                }
                _ => panic!("an unrecognized workflow must not bypass the pre-unlock retry oracle"),
            }
        }
        assert_eq!(requests.len(), 4);
        assert_eq!(provider_ids.len(), 2);
        assert_eq!(outcomes, [("http_rejected", 0), ("delayed", 1)]);
        assert!(owners.iter().all(|id| requests.contains(id)));
    }
}

use super::http::Receipt;
use hito_testkit::logs::LogCapture;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct Capture(LogCapture);

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        self.0.dispatch_with_spans(
            "off,hito_plugin_premiumize=trace,hito_debrid_premiumize=trace,hito_remote_audio=trace",
        )
    }

    pub fn assert_complete(&self, receipts: &[Receipt], length: u64) {
        let text = self.0.text();
        assert!(!text.is_empty());
        for private in [
            "owned-private",
            "http://",
            "https://",
            "http%3A",
            "https%3A",
            "Bearer ",
            "www.premiumize.me",
            "media.hito-fixture.invalid",
            super::provider::RECORDING,
            super::provider::SOURCE,
            super::provider::PEER_RECORDING,
            super::provider::PEER_SOURCE,
        ] {
            let escaped = serde_json::to_string(private).unwrap();
            assert!(!text.contains(private) && !text.contains(&escaped[1..escaped.len() - 1]));
        }
        let expected: BTreeMap<_, _> = receipts
            .iter()
            .map(|receipt| (receipt.request_id, receipt))
            .collect();
        assert_eq!(expected.len(), 11);
        let mut requests = BTreeSet::new();
        let mut provider_ids = BTreeSet::new();
        let mut counts = BTreeMap::new();
        let mut opening = None;
        let mut body_owner = None;
        let mut observations = Vec::new();
        for event in text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
        {
            keys(&event, &["level", "fields", "target", "span", "spans"]);
            assert_eq!(event["level"], "DEBUG");
            let fields = &event["fields"];
            assert!(fields["elapsed_ms"].as_u64().is_some());
            let spans = event["spans"].as_array().unwrap();
            assert!(!spans.is_empty());
            assert_eq!(&event["span"], spans.last().unwrap());
            for span in spans {
                keys(span, &["name", "request_id", "operation"]);
                assert_eq!(
                    Uuid::parse_str(span["request_id"].as_str().unwrap())
                        .unwrap()
                        .get_version_num(),
                    4
                );
                let operation = span["operation"].as_str().unwrap();
                assert_eq!(
                    span["name"],
                    match operation {
                        "manifest" | "search" | "detail" | "sources" | "prepare"
                        | "preparation_status" | "audio" => "premiumize_plugin_request",
                        "transfer_observation" => "premiumize_observation",
                        "bound_file_observation" => "premiumize_file_observation",
                        "bound_audio_opening" => "premiumize_audio",
                        _ => panic!("only static workflow operations are allowed"),
                    }
                );
            }
            let owner = spans
                .iter()
                .find(|span| span["name"] == "premiumize_plugin_request")
                .unwrap();
            let owner_id = Uuid::parse_str(owner["request_id"].as_str().unwrap()).unwrap();
            let receipt = expected
                .get(&owner_id)
                .expect("every workflow must retain a public HTTP correlation");
            assert_eq!(owner["operation"], receipt.operation);
            let name = fields["event"].as_str().unwrap();
            *counts.entry(name.to_owned()).or_insert(0_usize) += 1;
            match name {
                "premiumize_plugin_request_finished" => {
                    assert_eq!(event["target"], "hito_plugin_premiumize::http");
                    keys(fields, &["event", "outcome", "status", "elapsed_ms"]);
                    assert_eq!(fields["status"].as_u64(), Some(u64::from(receipt.status)));
                    assert_eq!(
                        fields["outcome"],
                        match receipt.status {
                            200 => "success",
                            401 => "unauthorized",
                            404 => "rejected",
                            _ => panic!("the public workflow returned an unexpected status"),
                        }
                    );
                    assert!(
                        requests.insert(owner_id),
                        "a request completed more than once"
                    );
                }
                "premiumize_plugin_audio_finished" | "remote_audio_body_finished" => {
                    assert_eq!(receipt.operation, "audio");
                    assert_eq!(fields["outcome"], "complete");
                    assert_eq!(fields["bytes"].as_u64(), Some(length));
                    if name == "remote_audio_body_finished" {
                        assert_eq!(event["target"], "hito_remote_audio::body");
                        keys(
                            fields,
                            &["event", "message", "outcome", "bytes", "elapsed_ms"],
                        );
                        assert_eq!(fields["message"], "Remote audio body finished");
                        body_owner = Some(event["span"]["request_id"].as_str().unwrap().to_owned());
                    } else {
                        assert_eq!(event["target"], "hito_plugin_premiumize::audio");
                        keys(fields, &["event", "outcome", "bytes", "elapsed_ms"]);
                    }
                }
                "premiumize_file_observation_finished"
                | "premiumize_observation_finished"
                | "premiumize_audio_opening_finished" => {
                    keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                    let (target, outcome, count) = match (name, receipt.operation) {
                        ("premiumize_file_observation_finished", "sources") => (
                            "hito_debrid_premiumize::file_observation",
                            "unsupported_state",
                            0_u64,
                        ),
                        ("premiumize_file_observation_finished", "preparation_status") => (
                            "hito_debrid_premiumize::file_observation",
                            "provider_completed",
                            1,
                        ),
                        ("premiumize_observation_finished", "prepare") => {
                            ("hito_debrid_premiumize::client", "queued", 1)
                        }
                        ("premiumize_observation_finished", "preparation_status") => {
                            ("hito_debrid_premiumize::client", "provider_completed", 1)
                        }
                        ("premiumize_audio_opening_finished", "audio") => {
                            ("hito_debrid_premiumize::audio", "complete", 1)
                        }
                        _ => panic!("a provider workflow ran outside its explicit HTTP action"),
                    };
                    assert_eq!(event["target"], target);
                    assert_eq!(fields["outcome"], outcome);
                    assert_eq!(fields["count"].as_u64(), Some(count));
                    let id = event["span"]["request_id"].as_str().unwrap().to_owned();
                    assert!(
                        provider_ids.insert(id.clone()),
                        "a provider workflow completed more than once"
                    );
                    observations.push((name.to_owned(), receipt.operation));
                    if name == "premiumize_audio_opening_finished" {
                        opening = Some(id);
                    }
                }
                _ => panic!("an unknown event bypassed the workflow and privacy contract"),
            }
        }
        assert_eq!(requests.len(), expected.len());
        assert_eq!(provider_ids.len(), 5);
        assert_eq!(observations.len(), 5);
        assert_eq!(opening, body_owner);
        assert_eq!(
            counts,
            BTreeMap::from([
                ("premiumize_plugin_request_finished".to_owned(), 11),
                ("premiumize_file_observation_finished".to_owned(), 2),
                ("premiumize_observation_finished".to_owned(), 2),
                ("premiumize_audio_opening_finished".to_owned(), 1),
                ("premiumize_plugin_audio_finished".to_owned(), 1),
                ("remote_audio_body_finished".to_owned(), 1),
            ])
        );
    }
}

fn keys(value: &Value, expected: &[&str]) {
    let actual: BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(actual, expected.iter().copied().collect());
}

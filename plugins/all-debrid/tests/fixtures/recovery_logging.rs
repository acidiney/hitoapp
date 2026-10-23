use super::{Capture, keys};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

impl Capture {
    pub fn assert_recovery_receipt(&self, private: &[&str], length: u64) {
        let text = self.0.text();
        assert!(!text.is_empty());
        for secret in private.iter().copied().chain([
            "http://",
            "https://",
            "http%3A",
            "https%3A",
            "Bearer ",
            "owned-private-media-cookie",
            "owned-private-upstream-name.wav",
            "owned-private-recovery-decoy",
            "owned-private-recovery-note",
        ]) {
            assert!(!secret.is_empty());
            let escaped = serde_json::to_string(secret).unwrap();
            assert!(!text.contains(secret) && !text.contains(&escaped[1..escaped.len() - 1]));
        }
        let events = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 36);
        let mut requests = BTreeSet::new();
        let mut owners = Vec::new();
        let mut statuses = Vec::new();
        let mut operations = Vec::new();
        let mut counts = BTreeMap::<(&str, &str), usize>::new();
        let mut provider_ids = BTreeSet::new();
        let mut openings = BTreeSet::new();
        let mut observation_openings = BTreeSet::new();
        let mut bodies = BTreeSet::new();
        let mut observations = BTreeMap::<&str, usize>::new();
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
                assert_eq!(
                    span["name"],
                    match span["operation"].as_str().unwrap() {
                        "prepare" | "preparation_status" | "audio" => "all_debrid_plugin_request",
                        "bound_file_preparation" => "all_debrid_preparation",
                        "delayed_file_poll" => "all_debrid_delayed_poll",
                        "magnet_observation" => "all_debrid_observation",
                        "bound_audio_opening" => "all_debrid_audio",
                        _ => panic!("only fixed safe recovery workflows are allowed"),
                    }
                );
            }
            let fields = &event["fields"];
            assert!(fields["elapsed_ms"].as_u64().is_some());
            let owner = &spans[0];
            assert_eq!(owner["name"], "all_debrid_plugin_request");
            let owner_id = Uuid::parse_str(owner["request_id"].as_str().unwrap()).unwrap();
            owners.push(owner_id);
            let name = fields["event"].as_str().unwrap();
            let outcome = fields["outcome"].as_str().unwrap();
            *counts.entry((name, outcome)).or_default() += 1;
            match name {
                "all_debrid_plugin_request_finished" => {
                    assert_eq!(event["target"], "hito_plugin_all_debrid::http");
                    assert_eq!(spans.len(), 1);
                    keys(fields, &["event", "outcome", "status", "elapsed_ms"]);
                    let status = fields["status"].as_u64().unwrap();
                    assert_eq!(outcome, if status == 200 { "success" } else { "rejected" });
                    assert!(requests.insert(owner_id));
                    statuses.push(status);
                    operations.push(owner["operation"].as_str().unwrap());
                }
                "all_debrid_plugin_audio_finished" => {
                    assert_eq!(event["target"], "hito_plugin_all_debrid::audio");
                    assert_eq!(spans.len(), 1);
                    assert_eq!(owner["operation"], "audio");
                    keys(fields, &["event", "outcome", "bytes", "elapsed_ms"]);
                    assert_eq!(
                        fields["bytes"].as_u64(),
                        Some(match outcome {
                            "cancelled" => 44,
                            "unavailable" => 0,
                            "complete" => length,
                            _ => panic!("the recovery tracer produced an unexpected body outcome"),
                        })
                    );
                }
                "remote_audio_body_finished" => {
                    assert_eq!(event["target"], "hito_remote_audio::body");
                    assert_eq!(spans.len(), 2);
                    assert_eq!(owner["operation"], "audio");
                    assert_eq!(spans[1]["name"], "all_debrid_audio");
                    assert_eq!(spans[1]["operation"], "bound_audio_opening");
                    keys(
                        fields,
                        &["event", "message", "outcome", "bytes", "elapsed_ms"],
                    );
                    assert_eq!(fields["message"], "Remote audio body finished");
                    assert_eq!(
                        fields["bytes"].as_u64(),
                        Some(match outcome {
                            "cancelled" => 44,
                            "complete" => length,
                            _ => panic!("the owned remote body produced an unexpected outcome"),
                        })
                    );
                    assert!(bodies.insert(spans[1]["request_id"].as_str().unwrap()));
                }
                "all_debrid_preparation_finished"
                | "all_debrid_delayed_poll_finished"
                | "all_debrid_observation_finished"
                | "all_debrid_audio_opening_finished" => {
                    assert_eq!(event["target"], "hito_debrid_all_debrid::client");
                    keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                    let is_open_observation =
                        name == "all_debrid_observation_finished" && owner["operation"] == "audio";
                    assert_eq!(spans.len(), if is_open_observation { 3 } else { 2 });
                    if is_open_observation {
                        assert_eq!(spans[1]["operation"], "bound_audio_opening");
                        assert!(
                            observation_openings.insert(spans[1]["request_id"].as_str().unwrap())
                        );
                    }
                    let provider = spans.last().unwrap();
                    let (span_name, operation, expected_outcome) = match name {
                        "all_debrid_preparation_finished" => (
                            "all_debrid_preparation",
                            "bound_file_preparation",
                            "delayed",
                        ),
                        "all_debrid_delayed_poll_finished" => {
                            ("all_debrid_delayed_poll", "delayed_file_poll", "candidate")
                        }
                        "all_debrid_observation_finished" => (
                            "all_debrid_observation",
                            "magnet_observation",
                            "provider_completed",
                        ),
                        _ => ("all_debrid_audio", "bound_audio_opening", outcome),
                    };
                    assert_eq!(provider["name"], span_name);
                    assert_eq!(provider["operation"], operation);
                    assert_eq!(outcome, expected_outcome);
                    assert_eq!(fields["count"].as_u64(), Some(u64::from(outcome != "busy")));
                    let id = provider["request_id"].as_str().unwrap();
                    assert_ne!(provider["request_id"], owner["request_id"]);
                    assert!(provider_ids.insert(id));
                    if name == "all_debrid_audio_opening_finished" && outcome == "complete" {
                        assert!(openings.insert(id));
                    }
                    if name == "all_debrid_observation_finished" {
                        *observations
                            .entry(owner["operation"].as_str().unwrap())
                            .or_default() += 1;
                    }
                }
                _ => panic!("an unrecognized event must not bypass the recovery privacy oracle"),
            }
        }
        assert_eq!(
            statuses,
            [200, 200, 200, 200, 200, 200, 200, 200, 503, 200, 200]
        );
        assert_eq!(
            operations,
            [
                "prepare",
                "prepare",
                "prepare",
                "preparation_status",
                "preparation_status",
                "preparation_status",
                "audio",
                "audio",
                "audio",
                "preparation_status",
                "audio"
            ]
        );
        assert_eq!(requests.len(), 11);
        assert_eq!(provider_ids.len(), 18);
        assert_eq!(openings, bodies);
        assert_eq!(openings, observation_openings);
        assert_eq!(
            observations,
            BTreeMap::from([("audio", 3), ("preparation_status", 4)])
        );
        assert_eq!(
            counts,
            BTreeMap::from([
                (("all_debrid_plugin_request_finished", "success"), 10),
                (("all_debrid_plugin_request_finished", "rejected"), 1),
                (("all_debrid_preparation_finished", "delayed"), 3),
                (("all_debrid_delayed_poll_finished", "candidate"), 4),
                (("all_debrid_observation_finished", "provider_completed"), 7),
                (("all_debrid_audio_opening_finished", "complete"), 3),
                (("all_debrid_audio_opening_finished", "busy"), 1),
                (("all_debrid_plugin_audio_finished", "cancelled"), 2),
                (("all_debrid_plugin_audio_finished", "unavailable"), 1),
                (("all_debrid_plugin_audio_finished", "complete"), 1),
                (("remote_audio_body_finished", "cancelled"), 2),
                (("remote_audio_body_finished", "complete"), 1),
            ])
        );
        assert!(owners.iter().all(|id| requests.contains(id)));
    }
}

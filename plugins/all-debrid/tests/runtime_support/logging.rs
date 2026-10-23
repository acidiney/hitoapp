use super::inputs::{PROVIDER_TOKEN, SERVICE_TOKEN, WRONG_TOKEN};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};
use uuid::Uuid;

pub fn assert_metadata_lifecycle(output: &str, directory: &Path) {
    for secret in [
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        WRONG_TOKEN,
        "owned-private-bootstrap-",
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS",
        "http://",
        "https://",
        "http%3A",
        "https%3A",
        directory.to_str().unwrap(),
    ] {
        let escaped = serde_json::to_string(secret).unwrap();
        assert!(
            !output.contains(secret) && !output.contains(&escaped[1..escaped.len() - 1]),
            "runtime logs must not expose operator or request input"
        );
    }
    let events = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        events.len(),
        9,
        "metadata bootstrap must emit only its exact lifecycle and request outcomes"
    );
    let mut lifecycle = Vec::new();
    let mut service_ids = BTreeSet::new();
    let mut requests = Vec::new();
    let mut request_ids = BTreeSet::new();
    for event in &events {
        keys(
            event,
            &["timestamp", "level", "fields", "target", "span", "spans"],
        );
        assert!(event["timestamp"].as_str().is_some());
        let fields = &event["fields"];
        assert!(fields["elapsed_ms"].as_u64().is_some());
        let spans = event["spans"].as_array().unwrap();
        assert_eq!(&event["span"], spans.last().unwrap());
        let name = fields["event"].as_str().unwrap();
        match name {
            "all_debrid_service_ready"
            | "all_debrid_shutdown_requested"
            | "all_debrid_service_stopped" => {
                assert_eq!(event["level"], "INFO");
                assert_eq!(event["target"], "hito_plugin_all_debrid::runtime");
                assert_eq!(spans.len(), 1);
                keys(&spans[0], &["name", "service_id"]);
                assert_eq!(spans[0]["name"], "all_debrid_service");
                let id = Uuid::parse_str(spans[0]["service_id"].as_str().unwrap()).unwrap();
                assert_eq!(id.get_version_num(), 4);
                service_ids.insert(id);
                if name == "all_debrid_shutdown_requested" {
                    keys(fields, &["event", "signal", "outcome", "elapsed_ms"]);
                    assert_eq!(fields["signal"], "SIGTERM");
                    assert_eq!(fields["outcome"], "draining");
                } else {
                    keys(fields, &["event", "outcome", "elapsed_ms"]);
                    assert_eq!(
                        fields["outcome"],
                        if name == "all_debrid_service_ready" {
                            "ready"
                        } else {
                            "stopped"
                        }
                    );
                }
                lifecycle.push(name);
            }
            "all_debrid_plugin_request_finished" => {
                assert_eq!(event["level"], "DEBUG");
                assert_eq!(event["target"], "hito_plugin_all_debrid::http");
                keys(fields, &["event", "outcome", "status", "elapsed_ms"]);
                assert!((1..=2).contains(&spans.len()));
                let owner = spans.last().unwrap();
                keys(owner, &["name", "request_id", "operation"]);
                assert_eq!(owner["name"], "all_debrid_plugin_request");
                let id = Uuid::parse_str(owner["request_id"].as_str().unwrap()).unwrap();
                assert_eq!(id.get_version_num(), 4);
                assert!(request_ids.insert(id));
                for parent in &spans[..spans.len() - 1] {
                    keys(parent, &["name", "service_id"]);
                    assert_eq!(parent["name"], "all_debrid_service");
                    let service = Uuid::parse_str(parent["service_id"].as_str().unwrap()).unwrap();
                    assert!(service_ids.contains(&service));
                }
                let status = fields["status"].as_u64().unwrap();
                assert_eq!(
                    fields["outcome"],
                    if status == 200 {
                        "success"
                    } else {
                        "unauthorized"
                    }
                );
                requests.push((owner["operation"].as_str().unwrap(), status));
            }
            _ => {
                panic!("metadata-only bootstrap must not enter another provider or media workflow")
            }
        }
    }
    assert_eq!(
        lifecycle,
        [
            "all_debrid_service_ready",
            "all_debrid_shutdown_requested",
            "all_debrid_service_stopped"
        ]
    );
    assert_eq!(service_ids.len(), 1);
    assert_eq!(
        requests,
        [
            ("manifest", 200),
            ("search", 401),
            ("search", 401),
            ("search", 401),
            ("search", 200),
            ("detail", 200)
        ]
    );
    assert_eq!(request_ids.len(), 6);
}

fn keys(value: &Value, expected: &[&str]) {
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        expected.iter().copied().collect()
    );
}

use super::{
    http::Receipt,
    inputs::{PROVIDER_TOKEN, SERVICE_TOKEN, WRONG_TOKEN},
};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};
use uuid::Uuid;

pub fn assert_metadata_lifecycle(output: &str, directory: &Path, receipts: &[Receipt]) {
    let directory = directory.to_string_lossy();
    for private in [
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        WRONG_TOKEN,
        "owned-private-premiumize-runtime-",
        "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk",
        "http://",
        "https://",
        "http%3A",
        "https%3A",
        &directory,
    ] {
        let escaped = serde_json::to_string(private)
            .unwrap_or_else(|_| panic!("the owned privacy sentinel must encode"));
        assert!(
            !output.contains(private) && !output.contains(&escaped[1..escaped.len() - 1]),
            "runtime logs must not expose operator files, credentials, request bodies or URLs"
        );
    }
    let events = output
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap_or_else(|_| {
                panic!("runtime output must contain only structured JSON events")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events.len(),
        9,
        "metadata startup must emit only lifecycle and HTTP outcomes"
    );
    let mut lifecycle = Vec::new();
    let mut service_ids = BTreeSet::new();
    let mut request_ids = BTreeSet::new();
    let mut requests = Vec::new();
    for event in &events {
        keys(
            event,
            &["timestamp", "level", "fields", "target", "span", "spans"],
        );
        assert!(
            event["timestamp"]
                .as_str()
                .is_some_and(|time| !time.is_empty() && time.len() <= 40)
        );
        let fields = &event["fields"];
        assert!(fields["elapsed_ms"].as_u64().is_some());
        let spans = event["spans"]
            .as_array()
            .expect("runtime events must include structured spans");
        assert!(
            !spans.is_empty()
                && event["span"]
                    == *spans
                        .last()
                        .expect("runtime events must have an owner span")
        );
        let name = fields["event"]
            .as_str()
            .expect("runtime events must use safe event names");
        match name {
            "premiumize_service_ready"
            | "premiumize_shutdown_requested"
            | "premiumize_service_stopped" => {
                assert!(
                    event["level"] == "INFO"
                        && event["target"] == "hito_plugin_premiumize::runtime"
                );
                assert_eq!(spans.len(), 1);
                service_ids.insert(service_id(&spans[0]));
                if name == "premiumize_shutdown_requested" {
                    keys(fields, &["event", "signal", "outcome", "elapsed_ms"]);
                    assert!(fields["signal"] == "SIGTERM" && fields["outcome"] == "draining");
                } else {
                    keys(fields, &["event", "outcome", "elapsed_ms"]);
                    assert!(
                        fields["outcome"]
                            == if name == "premiumize_service_ready" {
                                "ready"
                            } else {
                                "stopped"
                            }
                    );
                }
                lifecycle.push(name);
            }
            "premiumize_plugin_request_finished" => {
                assert!(
                    event["level"] == "DEBUG" && event["target"] == "hito_plugin_premiumize::http"
                );
                keys(fields, &["event", "outcome", "status", "elapsed_ms"]);
                assert!((1..=2).contains(&spans.len()));
                let owner = spans.last().expect("HTTP events must own a request span");
                keys(owner, &["name", "request_id", "operation"]);
                assert!(owner["name"] == "premiumize_plugin_request");
                let id = uuid(&owner["request_id"]);
                assert!(
                    request_ids.insert(id),
                    "each HTTP exchange must own a different correlation UUID"
                );
                for parent in &spans[..spans.len() - 1] {
                    assert!(
                        service_ids.contains(&service_id(parent)),
                        "HTTP parent spans must identify the same runtime"
                    );
                }
                let status = fields["status"]
                    .as_u64()
                    .expect("HTTP outcomes must contain a numeric status");
                assert!(
                    fields["outcome"]
                        == if status == 200 {
                            "success"
                        } else {
                            "unauthorized"
                        }
                );
                requests.push((
                    owner["operation"]
                        .as_str()
                        .expect("HTTP operations must be safe names"),
                    status,
                    id,
                ));
            }
            _ => panic!(
                "metadata-only startup must not enter provider, media or dependency workflows"
            ),
        }
    }
    assert!(
        lifecycle
            == [
                "premiumize_service_ready",
                "premiumize_shutdown_requested",
                "premiumize_service_stopped"
            ],
        "runtime lifecycle must finish the acknowledged SIGTERM in order"
    );
    assert_eq!(service_ids.len(), 1);
    let expected = receipts
        .iter()
        .map(|receipt| {
            (
                receipt.operation,
                u64::from(receipt.status),
                receipt.request_id,
            )
        })
        .collect::<Vec<_>>();
    assert!(
        requests == expected,
        "every HTTP response UUID must match its exact operation and outcome log"
    );
    assert_eq!(request_ids.len(), 6);
}

fn service_id(span: &Value) -> Uuid {
    keys(span, &["name", "service_id"]);
    assert!(span["name"] == "premiumize_service");
    uuid(&span["service_id"])
}

fn uuid(value: &Value) -> Uuid {
    let id = value
        .as_str()
        .and_then(|value| Uuid::parse_str(value).ok())
        .expect("correlation IDs must be safe UUIDs");
    assert_eq!(id.get_version_num(), 4);
    id
}

fn keys(value: &Value, expected: &[&str]) {
    let object = value
        .as_object()
        .expect("runtime fields must be structured objects");
    assert!(
        object.keys().map(String::as_str).collect::<BTreeSet<_>>()
            == expected.iter().copied().collect(),
        "runtime logs must contain only the exact safe fields"
    );
}

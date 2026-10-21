use super::{
    http::Receipt,
    inputs::{PROVIDER_TOKEN, SERVICE_TOKEN, WRONG_TOKEN},
};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};
use uuid::Uuid;

pub fn assert_missing_state(output: &str, directory: &Path) -> Uuid {
    let events = events(output, directory);
    assert_eq!(events.len(), 2);
    let rejected = events
        .iter()
        .find(|event| event["fields"]["event"] == "torbox_journal_open_finished")
        .expect("missing state must retain its rejected journal outcome");
    workflow(
        rejected,
        "hito_plugin_torbox::journal",
        "journal_open",
        "rejected",
        0,
    );
    let failed = events
        .iter()
        .find(|event| event["fields"]["event"] == "torbox_service_failed")
        .expect("missing state must retain its executable failure outcome");
    assert!(failed["level"] == "ERROR" && failed["target"] == "hito_plugin_torbox::runtime");
    keys(
        &failed["fields"],
        &["event", "outcome", "reason", "elapsed_ms"],
    );
    assert!(
        failed["fields"]["event"] == "torbox_service_failed"
            && failed["fields"]["outcome"] == "failed"
            && failed["fields"]["reason"] == "state_initialization_failed"
    );
    let spans = failed["spans"]
        .as_array()
        .expect("startup failure must retain its service owner");
    assert_eq!(spans.len(), 1);
    service_id(&spans[0])
}

pub fn assert_lifecycle(output: &str, directory: &Path, receipts: &[Receipt], held: bool) -> Uuid {
    let events = events(output, directory);
    assert_eq!(events.len(), 14 + usize::from(held));
    let mut owners = BTreeSet::new();
    let mut requests = Vec::new();
    let mut request_ids = BTreeSet::new();
    let mut startup = Vec::new();
    let mut timeline = Vec::new();
    let mut cancellations = 0;
    let mut shutdown = false;
    for event in &events {
        let fields = &event["fields"];
        let name = fields["event"]
            .as_str()
            .expect("events must have a closed safe name");
        match name {
            "torbox_candidate_authority_finished" => {
                assert!(
                    event["level"] == "DEBUG" && event["target"] == "hito_debrid_torbox::workflow"
                );
                keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                assert!(fields["outcome"] == "complete" && fields["count"] == 1);
                let spans = event["spans"]
                    .as_array()
                    .expect("authority derivation must own spans");
                assert_eq!(spans.len(), 2);
                workflow_span(&spans[0], "torbox_journal", "journal_open");
                workflow_span(
                    &spans[1],
                    "torbox_candidate_persistence",
                    "candidate_authority",
                );
                startup.push(name);
            }
            "torbox_journal_open_finished" | "torbox_journal_close_finished" => {
                let (operation, outcome) = if name == "torbox_journal_open_finished" {
                    ("journal_open", "ready")
                } else {
                    ("journal_close", "joined")
                };
                workflow(event, "hito_plugin_torbox::journal", operation, outcome, 0);
                if name == "torbox_journal_open_finished" {
                    startup.push(name);
                } else {
                    timeline.push(name);
                }
            }
            "torbox_service_ready"
            | "torbox_shutdown_requested"
            | "torbox_http_connections_joined"
            | "torbox_service_state_joined"
            | "torbox_service_stopped" => {
                assert!(event["level"] == "INFO");
                let target = if name == "torbox_http_connections_joined" {
                    "hito_plugin_torbox::runtime::http"
                } else {
                    "hito_plugin_torbox::runtime"
                };
                assert!(event["target"] == target);
                let spans = event["spans"]
                    .as_array()
                    .expect("lifecycle events must own service spans");
                assert_eq!(spans.len(), 1);
                owners.insert(service_id(&spans[0]));
                match name {
                    "torbox_shutdown_requested" => {
                        keys(fields, &["event", "signal", "outcome", "elapsed_ms"]);
                        assert!(fields["signal"] == "SIGTERM" && fields["outcome"] == "stopping");
                        shutdown = true;
                    }
                    "torbox_http_connections_joined" => {
                        keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                        assert!(fields["outcome"] == "joined");
                        let count = fields["count"]
                            .as_u64()
                            .expect("joined connection count must be numeric");
                        assert!((u64::from(held)..=16).contains(&count));
                    }
                    _ => {
                        keys(fields, &["event", "outcome", "elapsed_ms"]);
                        let outcome = match name {
                            "torbox_service_ready" => "ready",
                            "torbox_service_state_joined" => "joined",
                            _ => "stopped",
                        };
                        assert!(fields["outcome"] == outcome);
                    }
                }
                timeline.push(name);
            }
            "torbox_plugin_request_finished" => {
                assert!(
                    event["level"] == "DEBUG" && event["target"] == "hito_plugin_torbox::policy"
                );
                keys(fields, &["event", "outcome", "status", "elapsed_ms"]);
                let spans = event["spans"]
                    .as_array()
                    .expect("HTTP completion must retain its service owner");
                assert_eq!(spans.len(), 2);
                owners.insert(service_id(&spans[0]));
                keys(&spans[1], &["name", "request_id", "operation"]);
                assert!(spans[1]["name"] == "torbox_plugin_request");
                assert!(request_ids.insert(uuid(&spans[1]["request_id"])));
                let operation = spans[1]["operation"]
                    .as_str()
                    .expect("HTTP operation must be a safe name");
                let status = fields["status"]
                    .as_u64()
                    .expect("HTTP outcome must have a numeric status");
                if status == 0 {
                    assert!(
                        held && shutdown
                            && operation == "search"
                            && fields["outcome"] == "cancelled"
                    );
                    cancellations += 1;
                } else {
                    assert!(!shutdown, "metadata requests must finish before shutdown");
                    assert!(
                        fields["outcome"]
                            == if status == 200 {
                                "success"
                            } else {
                                "unauthorized"
                            }
                    );
                    requests.push((operation, status));
                }
            }
            _ => panic!("bootstrap must reject provider, media and unknown log classes"),
        }
    }
    assert!(
        startup
            == [
                "torbox_candidate_authority_finished",
                "torbox_journal_open_finished"
            ],
        "each successful startup must retain its exact authority and journal outcomes"
    );
    assert!(
        timeline
            == [
                "torbox_service_ready",
                "torbox_shutdown_requested",
                "torbox_http_connections_joined",
                "torbox_journal_close_finished",
                "torbox_service_state_joined",
                "torbox_service_stopped",
            ],
        "HTTP ownership must join before journal closure and service completion"
    );
    let expected = receipts
        .iter()
        .map(|receipt| (receipt.operation, u64::from(receipt.status)))
        .collect::<Vec<_>>();
    assert!(
        requests == expected,
        "HTTP logs must preserve every exact operation and status"
    );
    assert_eq!(request_ids.len(), 6 + usize::from(held));
    assert_eq!(cancellations, usize::from(held));
    assert_eq!(owners.len(), 1);
    *owners
        .first()
        .expect("each child must have one runtime service correlation")
}

pub(super) fn events(output: &str, directory: &Path) -> Vec<Value> {
    let directory = directory.to_string_lossy();
    for private in [
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        WRONG_TOKEN,
        "owned-private-torbox-runtime-",
        "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk",
        "http://",
        "https://",
        "http%3A",
        "https%3A",
        &directory,
    ] {
        let escaped = serde_json::to_string(private).expect("privacy sentinel must encode");
        assert!(
            !output.contains(private) && !output.contains(&escaped[1..escaped.len() - 1]),
            "runtime logs must not disclose credentials, key bytes, private paths, catalog data, bodies or URLs"
        );
    }
    output
        .lines()
        .map(|line| {
            let event: Value =
                serde_json::from_str(line).expect("runtime output must contain only JSON events");
            keys(
                &event,
                &["timestamp", "level", "fields", "target", "span", "spans"],
            );
            assert!(
                event["timestamp"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty() && value.len() <= 40)
            );
            assert!(
                event["fields"]["elapsed_ms"]
                    .as_u64()
                    .is_some_and(|value| value <= 30_000)
            );
            let spans = event["spans"]
                .as_array()
                .expect("runtime events must contain structured spans");
            assert!(
                !spans.is_empty()
                    && event["span"] == *spans.last().expect("events must have an owner")
            );
            event
        })
        .collect()
}

pub(super) fn workflow(event: &Value, target: &str, operation: &str, outcome: &str, count: u64) {
    assert!(event["level"] == "DEBUG" && event["target"] == target);
    keys(
        &event["fields"],
        &["event", "outcome", "count", "elapsed_ms"],
    );
    assert!(event["fields"]["outcome"] == outcome && event["fields"]["count"] == count);
    let spans = event["spans"]
        .as_array()
        .expect("journal outcomes must own a span");
    assert_eq!(spans.len(), 1);
    workflow_span(&spans[0], "torbox_journal", operation);
}

pub(super) fn workflow_span(span: &Value, name: &str, operation: &str) {
    keys(span, &["name", "request_id", "operation"]);
    assert!(span["name"] == name && span["operation"] == operation);
    uuid(&span["request_id"]);
}

pub(super) fn service_id(span: &Value) -> Uuid {
    keys(span, &["name", "service_id"]);
    assert!(span["name"] == "torbox_service");
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

pub(super) fn keys(value: &Value, expected: &[&str]) {
    let object = value
        .as_object()
        .expect("log fields must be structured objects");
    assert!(
        object.keys().map(String::as_str).collect::<BTreeSet<_>>()
            == expected.iter().copied().collect(),
        "logs must contain only exact safe fields"
    );
}

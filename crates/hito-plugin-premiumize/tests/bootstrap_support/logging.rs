use super::inputs::{PRIVATE, PROVIDER_TOKEN, SERVICE_TOKEN};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

pub fn assert_rejected(output: &str, directory: &Path, reason: &'static str) {
    let directory = directory.to_string_lossy();
    for private in [
        PRIVATE,
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        "kkkkkkkkkkkkkkkk",
        "xxxxxxxxxxxxxxxx",
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
            "bootstrap rejection must not log operator paths, credentials, arguments, environment or catalog input"
        );
    }
    let events = output
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap_or_else(|_| {
                panic!("bootstrap output must contain only structured JSON events")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events.len(),
        1,
        "rejected bootstrap must not bind, acknowledge readiness or enter another workflow"
    );
    let event = &events[0];
    keys(
        event,
        &["timestamp", "level", "fields", "target", "span", "spans"],
    );
    let timestamp = event["timestamp"]
        .as_str()
        .expect("bootstrap timestamps must be strings");
    assert!(
        (20..=40).contains(&timestamp.len())
            && timestamp.bytes().all(|byte| byte.is_ascii_digit()
                || matches!(byte, b'-' | b':' | b'.' | b'T' | b'Z' | b'+')),
        "bootstrap timestamps must use bounded safe time characters"
    );
    assert!(
        event["level"] == "ERROR" && event["target"] == "hito_plugin_premiumize::runtime",
        "bootstrap rejection must own the exact safe log level and target"
    );
    let fields = &event["fields"];
    keys(fields, &["event", "outcome", "reason", "elapsed_ms"]);
    assert!(
        fields["event"] == "premiumize_service_failed"
            && fields["outcome"] == "failed"
            && fields["reason"] == reason
            && fields["elapsed_ms"].as_u64().is_some(),
        "bootstrap rejection must contain only its expected closed reason and timing"
    );
    let spans = event["spans"]
        .as_array()
        .expect("bootstrap rejection must own structured spans");
    assert!(
        spans.len() == 1 && event["span"] == spans[0],
        "bootstrap rejection must own exactly its service span"
    );
    let span = &spans[0];
    keys(span, &["name", "service_id"]);
    assert!(
        span["name"] == "premiumize_service",
        "bootstrap span names must be fixed"
    );
    let service_id = span["service_id"]
        .as_str()
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
        .expect("bootstrap correlation IDs must be safe UUIDs");
    assert_eq!(service_id.get_version_num(), 4);
}

fn keys(value: &Value, expected: &[&str]) {
    let object = value
        .as_object()
        .expect("bootstrap fields must be structured objects");
    assert!(
        object.keys().map(String::as_str).collect::<BTreeSet<_>>()
            == expected.iter().copied().collect(),
        "bootstrap logs must contain only the exact safe fields"
    );
}

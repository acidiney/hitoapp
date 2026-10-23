use super::inputs::{PROVIDER_TOKEN, SERVICE_TOKEN};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};
use uuid::Uuid;

pub fn assert_rejected(output: &str, directory: &Path, reason: &str) {
    for secret in [
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        "owned-private-controls-",
        "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk",
        "http://",
        "https://",
        "http%3A",
        "https%3A",
        directory.to_str().unwrap(),
    ] {
        let escaped = serde_json::to_string(secret).unwrap();
        assert!(
            !output.contains(secret) && !output.contains(&escaped[1..escaped.len() - 1]),
            "bootstrap rejection must not log private operator input"
        );
    }
    let events = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        events.len(),
        1,
        "unsafe input must fail before Ready, binding or another workflow"
    );
    let event = &events[0];
    keys(
        event,
        &["timestamp", "level", "fields", "target", "span", "spans"],
    );
    assert!(event["timestamp"].as_str().is_some());
    assert_eq!(event["level"], "ERROR");
    assert_eq!(event["target"], "hito_plugin_all_debrid::runtime");
    let fields = &event["fields"];
    keys(fields, &["event", "outcome", "reason", "elapsed_ms"]);
    assert_eq!(fields["event"], "all_debrid_service_failed");
    assert_eq!(fields["outcome"], "failed");
    assert_eq!(fields["reason"], reason);
    assert!(fields["elapsed_ms"].as_u64().is_some());
    let spans = event["spans"].as_array().unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(&event["span"], &spans[0]);
    keys(&spans[0], &["name", "service_id"]);
    assert_eq!(spans[0]["name"], "all_debrid_service");
    assert_eq!(
        Uuid::parse_str(spans[0]["service_id"].as_str().unwrap())
            .unwrap()
            .get_version_num(),
        4
    );
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

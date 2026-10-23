use super::{fixture::Provider, http};
use serde_json::Value;
use std::net::SocketAddr;

pub(super) async fn assert_status(
    address: SocketAddr,
    token: &str,
    retained: &Value,
    revision: u64,
    matches_source_choice: bool,
    providers: [&Provider; 2],
) {
    assert!(
        retained["status"] == "ready"
            && retained["intent_id"].is_string()
            && retained["source_id"].is_string(),
        "the status comparison must start from an actual ready retained copy"
    );
    let recording = retained["recording_id"]
        .as_str()
        .unwrap_or_else(|| panic!("the retained status must identify its recording"));
    let before = providers.map(|provider| provider.requests());
    let status = http::get(
        address,
        token,
        &format!("/api/v1/recordings/{recording}/download"),
    )
    .await;
    assert!(
        status["status"] == "ready",
        "changing the source choice must not relabel a retained ready transfer"
    );
    for field in [
        "intent_id",
        "recording_id",
        "source_id",
        "host_device_id",
        "bytes_received",
        "total_bytes",
        "error",
    ] {
        assert!(
            status[field] == retained[field],
            "reading download eligibility must preserve the same retained copy and transfer facts"
        );
    }
    assert!(
        status["choice_revision"].as_u64() == Some(revision),
        "download status must report the current source-choice revision"
    );
    assert!(
        status["matches_source_choice"].as_bool() == Some(matches_source_choice),
        "download status must distinguish retained bytes from eligibility for the current source choice"
    );
    assert_eq!(
        providers.map(|provider| provider.requests()),
        before,
        "reading retained download status must not attempt any provider request or implicit download"
    );
}

#![cfg(target_os = "linux")]

#[path = "support/runtime.rs"]
mod runtime;

use runtime::{Host, PendingProvision};
use serde_json::{Value, json};
use std::{
    fs::{OpenOptions, Permissions},
    io::Write,
    net::TcpStream,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    time::Duration,
};

const ADMIN_TOKEN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";

#[test]
fn private_bootstrap_drains_authenticated_http_before_shutdown_and_preserves_reporters_on_restart()
{
    assert_eq!(ADMIN_TOKEN.len(), 64);
    let fixture = tempfile::tempdir().unwrap();
    std::fs::set_permissions(fixture.path(), Permissions::from_mode(0o700)).unwrap();
    let credential_path = fixture.path().join("private-community-admin-token");
    let state_directory = fixture.path().join("private-community-state");
    let mut credential = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&credential_path)
        .unwrap();
    writeln!(credential, "{ADMIN_TOKEN}").unwrap();
    drop(credential);
    std::fs::set_permissions(&credential_path, Permissions::from_mode(0o400)).unwrap();
    let metadata = credential_path.metadata().unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(metadata.mode() & 0o7777, 0o400);

    let mut host = Host::start(fixture.path(), &state_directory, &credential_path, "first");
    let document = host.ready();
    assert!(document["openapi"].as_str().unwrap().starts_with("3."));
    assert!(document["paths"]["/v1/admin/reporters"]["post"].is_object());
    let (status, rejection) = host
        .request(
            "POST",
            "/v1/admin/reporters",
            Some(ADMIN_TOKEN),
            r#"{"private-community-body-key":"private-community-body-value"}"#,
        )
        .unwrap();
    assert_eq!(status, 422);
    assert_eq!(
        serde_json::from_slice::<Value>(&rejection).unwrap(),
        json!({"error": "invalid_input"})
    );

    let payload = "{}\n";
    let mut pending = PendingProvision::start(host.address, ADMIN_TOKEN, payload);
    host.signal_term();
    host.wait_event("community_shutdown_requested");
    assert!(
        host.status().is_none(),
        "Community exited with an unread authenticated request body"
    );
    let (status, receipt) = pending.finish(payload).unwrap();
    assert_eq!(status, 201);
    let reporter: Value = serde_json::from_slice(&receipt).unwrap();
    uuid::Uuid::parse_str(reporter["reporter_id"].as_str().unwrap()).unwrap();
    let reporter_token = reporter["token"].as_str().unwrap().to_owned();
    assert_eq!(reporter_token.len(), 64);
    assert!(host.wait_exit().success());
    assert!(TcpStream::connect_timeout(&host.address, Duration::from_millis(100)).is_err());
    let first_events = captured_events(&host, fixture.path(), &reporter_token);
    let index = |name: &str| {
        first_events
            .iter()
            .position(|event| event["fields"]["event"] == name)
            .unwrap()
    };
    let ready = index("community_service_ready");
    let requested = index("community_shutdown_requested");
    let closed = index("community_state_closed");
    let state_closed = index("community_service_state_closed");
    let stopped = index("community_service_stopped");
    assert!(
        ready < requested && requested < closed && closed < state_closed && state_closed < stopped
    );
    let first_service_id = lifecycle_service_id(&first_events);
    assert_eq!(first_events[requested]["fields"]["signal"], "SIGTERM");
    assert_eq!(first_events[requested]["fields"]["outcome"], "draining");
    assert_eq!(first_events[stopped]["fields"]["outcome"], "stopped");
    assert!(
        first_events.iter().enumerate().any(|(index, event)| {
            index > requested
                && index < closed
                && event["fields"]["event"] == "community_http_response"
                && event["fields"]["status"] == 201
        }),
        "The accepted request must finish before state shutdown"
    );

    let mut restarted = Host::start(
        fixture.path(),
        &state_directory,
        &credential_path,
        "restarted",
    );
    restarted.ready();
    let outcome = json!({
        "submission_id": "37729b20-c721-468a-8473-bc4783b75c9f",
        "source": {"provider": "youtube", "recording_id": "PrivTone001"},
        "outcome": "successful_playback"
    })
    .to_string();
    let (status, receipt) = restarted
        .request("POST", "/v1/outcomes", Some(&reporter_token), &outcome)
        .unwrap();
    assert_eq!(status, 201);
    assert_eq!(
        serde_json::from_slice::<Value>(&receipt).unwrap()["status"],
        "accepted"
    );
    restarted.signal_term();
    assert!(restarted.wait_exit().success());
    assert!(TcpStream::connect_timeout(&restarted.address, Duration::from_millis(100)).is_err());
    let restarted_events = captured_events(&restarted, fixture.path(), &reporter_token);
    assert!(
        restarted_events
            .iter()
            .any(|event| event["fields"]["event"] == "community_state_closed")
    );
    let restarted_index = |name: &str| {
        restarted_events
            .iter()
            .position(|event| event["fields"]["event"] == name)
            .unwrap()
    };
    assert!(
        restarted_index("community_state_closed")
            < restarted_index("community_service_state_closed")
            && restarted_index("community_service_state_closed")
                < restarted_index("community_service_stopped")
    );
    assert_ne!(first_service_id, lifecycle_service_id(&restarted_events));
}

fn lifecycle_service_id(events: &[Value]) -> uuid::Uuid {
    let mut instance = None;
    for name in [
        "community_service_ready",
        "community_shutdown_requested",
        "community_service_state_closed",
        "community_service_stopped",
    ] {
        let event = events
            .iter()
            .find(|event| event["fields"]["event"] == name)
            .expect("Community runtime lifecycle event missing");
        let span = event["spans"]
            .as_array()
            .expect("Community runtime instance spans missing")
            .iter()
            .find(|span| span["name"] == "community_service")
            .expect("Community runtime instance correlation missing");
        let service_id = uuid::Uuid::parse_str(
            span["service_id"]
                .as_str()
                .expect("Community runtime instance UUID missing"),
        )
        .expect("Community runtime instance correlation was not a UUID");
        match instance {
            Some(instance) => assert_eq!(service_id, instance),
            None => instance = Some(service_id),
        }
    }
    instance.unwrap()
}

fn captured_events(host: &Host, directory: &std::path::Path, reporter_token: &str) -> Vec<Value> {
    let output = host.logs();
    for forbidden in [
        ADMIN_TOKEN,
        reporter_token,
        "private-",
        "PrivTone001",
        directory.to_str().unwrap(),
    ] {
        assert!(
            !output.contains(forbidden),
            "Community logs exposed private fixture input"
        );
    }
    output
        .lines()
        .map(|line| {
            serde_json::from_str(line).expect("Community runtime must emit structured JSON logs")
        })
        .collect()
}

#![cfg(target_os = "linux")]

#[path = "support/runtime.rs"]
mod runtime;

use runtime::{Host, PendingRequest};
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
    let mut pending =
        PendingRequest::start(host.address, "/v1/admin/reporters", ADMIN_TOKEN, payload);
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

#[test]
fn revocation_fences_an_already_authenticated_unread_outcome_without_erasing_peer_state() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::set_permissions(fixture.path(), Permissions::from_mode(0o700)).unwrap();
    let credential_path = fixture.path().join("private-community-revocation-token");
    let state_directory = fixture.path().join("private-community-revocation-state");
    let mut credential = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&credential_path)
        .unwrap();
    writeln!(credential, "{ADMIN_TOKEN}").unwrap();
    drop(credential);
    std::fs::set_permissions(&credential_path, Permissions::from_mode(0o400)).unwrap();
    let private_bytes = std::fs::read(&credential_path).unwrap();
    let private_metadata = credential_path.metadata().unwrap();
    let mut host = Host::start(
        fixture.path(),
        &state_directory,
        &credential_path,
        "revocation",
    );
    host.ready();
    let (status, reporter) = request_json(
        &host,
        "POST",
        "/v1/admin/reporters",
        Some(ADMIN_TOKEN),
        "{}",
    );
    assert_eq!(status, 201);
    let token = reporter["token"].as_str().unwrap();
    let reporter_id = reporter["reporter_id"].as_str().unwrap();
    let (status, peer) = request_json(
        &host,
        "POST",
        "/v1/admin/reporters",
        Some(ADMIN_TOKEN),
        "{}",
    );
    assert_eq!(status, 201);
    let peer_token = peer["token"].as_str().unwrap();
    let accepted = json!({
        "submission_id": "dc601464-f2ec-42bf-b9b5-4509cbe04480",
        "source": {"provider": "jamendo", "recording_id": "18446744073709551615"},
        "outcome": "successful_playback"
    })
    .to_string();
    assert_eq!(
        request_json(&host, "POST", "/v1/outcomes", Some(token), &accepted).0,
        201
    );
    let (status, peer_receipt) =
        request_json(&host, "POST", "/v1/outcomes", Some(peer_token), &accepted);
    assert_eq!(status, 201);
    let held = json!({
        "submission_id": "bccdbd9e-05d9-41de-b88e-c70df2ab7bb6",
        "source": {"provider": "youtube", "recording_id": "PrivTone001"},
        "outcome": "successful_playback"
    })
    .to_string();
    for _ in 1..300 {
        let mut submission: Value = serde_json::from_str(&accepted).unwrap();
        submission["submission_id"] = json!(uuid::Uuid::new_v4());
        assert_eq!(
            request_json(
                &host,
                "POST",
                "/v1/outcomes",
                Some(token),
                &submission.to_string()
            )
            .0,
            201
        );
    }
    assert_eq!(
        request_json(&host, "POST", "/v1/outcomes", Some(token), &held),
        (429, json!({"error": "rate_limited"}))
    );
    let mut pending = PendingRequest::start(host.address, "/v1/outcomes", token, &held);
    let path = format!("/v1/admin/reporters/{reporter_id}");
    let (status, body) = request_json(&host, "DELETE", &path, Some(ADMIN_TOKEN), "");
    assert_eq!(status, 204);
    assert_eq!(body, Value::Null);
    let (status, rejection) = pending.finish(&held).unwrap();
    assert_eq!(status, 401);
    assert_eq!(
        serde_json::from_slice::<Value>(&rejection).unwrap(),
        json!({"error": "unauthorized"})
    );
    assert_eq!(
        request_json(&host, "POST", "/v1/outcomes", Some(token), &held),
        (401, json!({"error": "unauthorized"}))
    );
    assert_eq!(
        request_json(&host, "POST", "/v1/outcomes", Some(peer_token), &accepted),
        (200, peer_receipt)
    );
    let candidates = json!({"candidates": [
        {"provider": "jamendo", "recording_id": "18446744073709551615"},
        {"provider": "youtube", "recording_id": "PrivTone001"}
    ]})
    .to_string();
    let (status, ranking) = request_json(&host, "POST", "/v1/rankings", None, &candidates);
    assert_eq!(status, 200);
    assert_eq!(
        ranking,
        json!({"ranked": [
            {"source": {"provider": "jamendo", "recording_id": "18446744073709551615"}, "successful_reporters": 1},
            {"source": {"provider": "youtube", "recording_id": "PrivTone001"}, "successful_reporters": 0}
        ]})
    );
    host.signal_term();
    assert!(host.wait_exit().success());
    assert!(
        std::fs::read(&credential_path).unwrap() == private_bytes,
        "Revocation changed the administrator credential file"
    );
    let retained = credential_path.metadata().unwrap();
    assert_eq!(
        (
            retained.dev(),
            retained.ino(),
            retained.mode(),
            retained.uid(),
            retained.gid(),
            retained.nlink(),
            retained.len()
        ),
        (
            private_metadata.dev(),
            private_metadata.ino(),
            private_metadata.mode(),
            private_metadata.uid(),
            private_metadata.gid(),
            private_metadata.nlink(),
            private_metadata.len()
        )
    );
    let events = captured_events(&host, fixture.path(), token);
    assert!(events.iter().any(
        |event| event["fields"]["event"] == "community_request_rejected"
            && event["fields"]["outcome"] == "rate_limited"
            && event["fields"]["status"] == 429
    ));
    let logs = host.logs();
    for forbidden in [
        peer_token,
        reporter_id,
        peer["reporter_id"].as_str().unwrap(),
        "18446744073709551615",
        "dc601464-f2ec-42bf-b9b5-4509cbe04480",
        "bccdbd9e-05d9-41de-b88e-c70df2ab7bb6",
    ] {
        assert!(
            !logs.contains(forbidden),
            "Community revocation logs exposed fixture input"
        );
    }
    let revoked = events
        .iter()
        .find(|event| event["fields"]["event"] == "community_reporter_revoked")
        .expect("Revocation workflow log missing");
    assert_eq!(revoked["fields"]["outcome"], "removed");
    assert_eq!(revoked["fields"]["reporter_count"], 1);
    let request = revoked["spans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|span| span["name"] == "community_http_request")
        .unwrap();
    assert_eq!(request["route"], "/v1/admin/reporters/{reporter_id}");
    assert_eq!(request["method"], "DELETE");
    assert_eq!(
        uuid::Uuid::parse_str(request["request_id"].as_str().unwrap())
            .unwrap()
            .get_version_num(),
        4
    );
    assert!(events.iter().any(
        |event| event["fields"]["event"] == "community_http_response"
            && event["fields"]["status"] == 401
    ));
}

fn request_json(
    host: &Host,
    method: &str,
    path: &str,
    token: Option<&str>,
    payload: &str,
) -> (u16, Value) {
    let (status, body) = host.request(method, path, token, payload).unwrap();
    (
        status,
        if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    )
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

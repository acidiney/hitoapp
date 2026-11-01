#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[path = "support/runtime.rs"]
mod runtime;

use hito_plugin_sdk::{Capability, ContentPolicy, Manifest, SourcePersistence};
use runtime::{Host, PendingRequest};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::Write,
    net::TcpStream,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    time::Duration,
};

const CLIENT_ID: &str = "private-runtime-jamendo-client";

#[test]
fn runtime_manifest_rejections_and_joined_shutdown_keep_operator_inputs_private() {
    let directory = tempfile::tempdir().expect("runtime directory setup failed");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("runtime directory protection failed");
    assert_eq!(
        directory.path().metadata().unwrap().permissions().mode() & 0o077,
        0
    );
    let credential_path = directory.path().join("private-operator-client-id");
    let mut credential = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&credential_path)
        .expect("runtime credential setup failed");
    writeln!(credential, "{CLIENT_ID}").expect("runtime credential write failed");
    drop(credential);
    let mut host = Host::start(directory.path(), &credential_path);
    let (status, body) = host.manifest();
    assert_eq!(status, 200);
    let manifest: Manifest = serde_json::from_slice(&body).expect("runtime manifest JSON failed");
    manifest
        .validate()
        .expect("runtime manifest contract failed");
    assert_eq!(manifest.id, "org.hito.jamendo");
    assert_eq!(
        manifest.capabilities,
        [
            Capability::Search,
            Capability::Sources,
            Capability::Playlists,
            Capability::Featured
        ]
        .into()
    );
    assert!(manifest.authorization.is_none());
    for path in ["/v1/download", "/v1/cache"] {
        for method in ["GET", "POST"] {
            let (status, _) = host
                .request(method, path, "")
                .expect("runtime unsupported persistence request failed");
            assert_eq!(status, 404);
        }
    }
    assert_eq!(
        manifest.content_policy,
        Some(ContentPolicy {
            source_persistence: SourcePersistence::PlaybackOnly,
        })
    );

    let invalid_search = json!({
        "query": "private-runtime-search-query", "cursor": null, "limit": 0,
    })
    .to_string();
    for (method, path, payload) in [
        (
            "POST",
            "/v1/search?private-uri-query=private-uri-value",
            invalid_search.as_str(),
        ),
        (
            "POST",
            "/v1/recordings/detail?private-uri-query=private-uri-value",
            r#"{"recording_id":"private-runtime-recording-id",private-malformed-body}"#,
        ),
        (
            "GET",
            "/v1/audio?source_id=private-runtime-source-id&private-query=private-value",
            "",
        ),
    ] {
        let (status, body) = host
            .request(method, path, payload)
            .expect("runtime rejection request failed");
        assert_eq!(status, 400);
        assert_eq!(
            serde_json::from_slice::<Value>(&body).expect("runtime rejection JSON failed"),
            json!({"error": "The Jamendo request is invalid"})
        );
    }

    let mut pending = PendingRequest::start(host.address, &invalid_search);
    host.signal_term();
    host.wait_event("jamendo_shutdown_requested");
    assert!(
        host.status().is_none(),
        "runtime exited before its active request drained"
    );
    let (status, body) = pending
        .finish(&invalid_search)
        .expect("runtime drain response failed");
    assert_eq!(status, 400);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).expect("runtime drain JSON failed"),
        json!({"error": "The Jamendo request is invalid"})
    );
    assert!(
        host.wait_exit().success(),
        "runtime did not finish joined shutdown"
    );
    assert!(
        TcpStream::connect_timeout(&host.address, Duration::from_millis(100)).is_err(),
        "runtime left its listener available after exit"
    );

    let output = host.logs();
    for forbidden in [
        CLIENT_ID,
        "private-",
        "http://",
        "https://",
        directory.path().to_str().unwrap(),
        credential_path.to_str().unwrap(),
    ] {
        assert!(
            !output.contains(forbidden),
            "runtime logs exposed operator or request input"
        );
    }
    assert!(
        !output.contains("jamendo_provider_complete"),
        "manifest or invalid input used the provider workflow"
    );
    assert!(
        !output.contains("jamendo_audio_open_complete"),
        "manifest or invalid input opened audio"
    );
    let events: Vec<Value> = output
        .lines()
        .map(|line| {
            serde_json::from_str(line).expect("runtime stdout/stderr was not structured JSON")
        })
        .collect();
    let index = |name: &str| {
        events
            .iter()
            .position(|event| event["fields"]["event"] == name)
            .expect("runtime lifecycle event missing")
    };
    let ready = index("jamendo_service_ready");
    let requested = index("jamendo_shutdown_requested");
    let stopped = index("jamendo_service_stopped");
    assert!(ready < requested && requested < stopped);
    assert_eq!(events[requested]["fields"]["signal"], "SIGTERM");
    assert_eq!(events[requested]["fields"]["outcome"], "draining");
    assert_eq!(events[stopped]["fields"]["outcome"], "stopped");
    for event in [&events[ready], &events[requested], &events[stopped]] {
        assert_eq!(event["level"], "INFO");
        assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
    }
    let mut correlations = HashSet::new();
    let mut operations = HashSet::new();
    let mut drained_after_signal = false;
    for (index, event) in events
        .iter()
        .enumerate()
        .filter(|(_, event)| event["fields"]["event"] == "jamendo_request_complete")
    {
        let span = event["spans"]
            .as_array()
            .expect("runtime request spans missing")
            .iter()
            .find(|span| span["name"] == "jamendo_request")
            .expect("runtime request correlation missing");
        let request_id = span["request_id"]
            .as_str()
            .expect("runtime request UUID missing");
        uuid::Uuid::parse_str(request_id).expect("runtime request correlation was not a UUID");
        assert!(
            correlations.insert(request_id.to_owned()),
            "runtime reused a request correlation"
        );
        assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
        let operation = span["operation"]
            .as_str()
            .expect("runtime request operation missing");
        let status = event["fields"]["status"]
            .as_u64()
            .expect("runtime request status missing");
        operations.insert((operation, status));
        if index > requested && index < stopped && operation == "search" && status == 400 {
            drained_after_signal = true;
        }
    }
    for expected in [
        ("manifest", 200),
        ("search", 400),
        ("detail", 400),
        ("audio", 400),
        ("unknown", 404),
    ] {
        assert!(
            operations.contains(&expected),
            "runtime safe request outcome missing"
        );
    }
    assert!(
        drained_after_signal,
        "runtime stopped before logging the drained request"
    );
}

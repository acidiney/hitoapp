#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[path = "support/runtime.rs"]
mod runtime;
#[path = "support/runtime_inputs.rs"]
mod runtime_inputs;

use hito_plugin_sdk::{Capability, Manifest, ProviderRecording, SearchPage, SourcePersistence};
use runtime::{Host, PendingRequest};
use runtime_inputs::{Inputs, PROVIDER_TOKEN, RECORDING, SERVICE_TOKEN, TITLE, WRONG_TOKEN};
use serde_json::{Value, json};
use std::{collections::HashSet, net::TcpStream, time::Duration};

#[test]
fn private_catalog_runtime_drains_authenticated_http_and_joins_without_exposing_operator_inputs() {
    let directory = tempfile::tempdir().expect("runtime directory setup failed");
    let inputs = Inputs::create(directory.path());
    let mut host = Host::start(
        directory.path(),
        &inputs.provider,
        &inputs.service,
        &inputs.catalog,
    );
    host.wait_event("real_debrid_service_ready");

    let (status, body) = host
        .request(
            "GET",
            "/v1/manifest?private-runtime-query=private-runtime-value",
            None,
            "",
        )
        .expect("runtime public manifest failed");
    assert_eq!(status, 200);
    let manifest: Manifest = serde_json::from_slice(&body).expect("runtime manifest JSON failed");
    manifest
        .validate()
        .expect("runtime manifest contract failed");
    assert_eq!(manifest.id, "org.hito.real-debrid");
    assert_eq!(
        manifest.capabilities,
        [Capability::Search, Capability::Sources].into()
    );
    assert_eq!(
        manifest.content_policy.unwrap().source_persistence,
        SourcePersistence::PlaybackOnly
    );

    let search = json!({"query": TITLE, "cursor": null, "limit": 1}).to_string();
    for token in [None, Some(WRONG_TOKEN)] {
        let (status, body) = host
            .request("POST", "/v1/search", token, &search)
            .expect("runtime authentication request failed");
        assert_eq!(status, 401);
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({
                "error": "Plugin-service authentication is required", "reason": "unauthorized",
            })
        );
    }
    let (status, body) = host
        .request(
            "POST",
            "/v1/search?private-runtime-query=private-runtime-value",
            Some(SERVICE_TOKEN),
            &search,
        )
        .expect("runtime authenticated search failed");
    assert_eq!(status, 200);
    let page: SearchPage = serde_json::from_slice(&body).expect("runtime search JSON failed");
    page.validate(1)
        .expect("runtime search SDK contract failed");
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        serde_json::to_value(&page.items[0]).unwrap(),
        inputs.recording
    );
    assert!(page.next_cursor.is_none());

    let (status, body) = host
        .request(
            "POST",
            "/v1/recordings/detail",
            Some(SERVICE_TOKEN),
            &json!({"recording_id": RECORDING}).to_string(),
        )
        .expect("runtime recording detail failed");
    assert_eq!(status, 200);
    let recording: ProviderRecording =
        serde_json::from_slice(&body).expect("runtime recording JSON failed");
    recording
        .validate()
        .expect("runtime recording SDK contract failed");
    assert_eq!(serde_json::to_value(recording).unwrap(), inputs.recording);

    let unknown = json!({"recording_id": "private-runtime-unknown-recording"}).to_string();
    for (method, path, payload) in [
        ("POST", "/v1/recordings/detail", unknown.as_str()),
        ("POST", "/v1/sources", unknown.as_str()),
        (
            "GET",
            "/v1/audio?source_id=private-runtime-unknown-source",
            "",
        ),
    ] {
        let (status, body) = host
            .request(method, path, Some(SERVICE_TOKEN), payload)
            .expect("runtime unknown identity request failed");
        assert_eq!(status, 404);
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({
                "error": "The curated recording or source was not found", "reason": "not_found",
            })
        );
    }

    let malformed = r#"{"query":"private-runtime-held-body",private-runtime-malformed-body}"#;
    let mut pending = PendingRequest::start(host.address, SERVICE_TOKEN, malformed);
    host.signal_term();
    host.wait_event("real_debrid_shutdown_requested");
    assert!(
        host.status().is_none(),
        "runtime exited before its accepted request drained"
    );
    let (status, body) = pending
        .finish(malformed)
        .expect("runtime drain response failed");
    assert_eq!(status, 400);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({
            "error": "The plugin request is invalid", "reason": "invalid_request",
        })
    );
    assert!(
        host.wait_exit().success(),
        "runtime did not join graceful shutdown"
    );
    assert!(
        TcpStream::connect_timeout(&host.address, Duration::from_millis(100)).is_err(),
        "runtime listener remained available after exit"
    );
    inputs.assert_preserved(directory.path());
    assert_safe_lifecycle(&host.logs(), directory.path().to_str().unwrap());
}

fn assert_safe_lifecycle(output: &str, directory: &str) {
    for forbidden in [
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        WRONG_TOKEN,
        "private-runtime-",
        "http://",
        "https://",
        directory,
    ] {
        assert!(
            !output.contains(forbidden),
            "runtime logs exposed operator or request input"
        );
    }
    for forbidden in [
        "real_debrid_observation_finished",
        "real_debrid_unrestriction_finished",
        "real_debrid_audio_opening_finished",
    ] {
        assert!(
            !output.contains(forbidden),
            "metadata-only runtime requests entered an upstream workflow"
        );
    }
    let events: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).expect("runtime output was not structured JSON"))
        .collect();
    let index = |name: &str| {
        events
            .iter()
            .position(|event| event["fields"]["event"] == name)
            .expect("runtime lifecycle event missing")
    };
    let ready = index("real_debrid_service_ready");
    let requested = index("real_debrid_shutdown_requested");
    let stopped = index("real_debrid_service_stopped");
    assert!(ready < requested && requested < stopped);
    assert_eq!(events[requested]["fields"]["signal"], "SIGTERM");
    for (event, outcome) in [
        (&events[ready], "ready"),
        (&events[requested], "draining"),
        (&events[stopped], "stopped"),
    ] {
        assert_eq!(event["level"], "INFO");
        assert_eq!(event["fields"]["outcome"], outcome);
        assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
    }
    let service_id = |event: &Value| {
        event["spans"]
            .as_array()
            .expect("runtime service spans missing")
            .iter()
            .find(|span| span["name"] == "real_debrid_service")
            .and_then(|span| span["service_id"].as_str())
            .expect("runtime service UUID missing")
            .to_owned()
    };
    let id = service_id(&events[ready]);
    uuid::Uuid::parse_str(&id).expect("runtime service correlation was not a UUID");
    assert_eq!(service_id(&events[requested]), id);
    assert_eq!(service_id(&events[stopped]), id);
    let mut correlations = HashSet::new();
    let mut outcomes = HashSet::new();
    let mut drained = false;
    let mut completed = 0;
    for (index, event) in events
        .iter()
        .enumerate()
        .filter(|(_, event)| event["fields"]["event"] == "real_debrid_plugin_request_finished")
    {
        completed += 1;
        let span = event["spans"]
            .as_array()
            .expect("runtime request spans missing")
            .iter()
            .find(|span| span["name"] == "real_debrid_plugin_request")
            .expect("runtime request span missing");
        let request_id = span["request_id"]
            .as_str()
            .expect("runtime request UUID missing");
        uuid::Uuid::parse_str(request_id).expect("runtime request correlation was not a UUID");
        assert!(
            correlations.insert(request_id),
            "runtime reused a request correlation"
        );
        let operation = span["operation"]
            .as_str()
            .expect("runtime request operation missing");
        let status = event["fields"]["status"]
            .as_u64()
            .expect("runtime request status missing");
        let outcome = event["fields"]["outcome"]
            .as_str()
            .expect("runtime safe request outcome missing");
        assert_eq!(
            outcome,
            match status {
                200 => "success",
                401 => "unauthorized",
                _ => "rejected",
            }
        );
        assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
        outcomes.insert((operation, status));
        drained |= index > requested && index < stopped && operation == "search" && status == 400;
    }
    assert_eq!(completed, 9, "runtime lost an accepted request outcome");
    for expected in [
        ("manifest", 200),
        ("search", 401),
        ("search", 200),
        ("detail", 200),
        ("detail", 404),
        ("sources", 404),
        ("audio", 404),
        ("search", 400),
    ] {
        assert!(
            outcomes.contains(&expected),
            "runtime safe request outcome missing"
        );
    }
    assert!(
        drained,
        "runtime stopped before logging the drained request"
    );
}

#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[path = "runtime_support/http.rs"]
mod http;
#[path = "runtime_support/inputs.rs"]
mod inputs;
#[path = "runtime_support/logging.rs"]
mod logging;
#[path = "runtime_support/process.rs"]
mod process;

use hito_plugin_sdk::{Capability, Manifest, ProviderRecording, SearchPage, SourcePersistence};
use inputs::{Inputs, PROVIDER_TOKEN, RECORDING, SERVICE_TOKEN, TITLE, WRONG_TOKEN};
use serde_json::{Value, json};
use std::{net::TcpStream, panic::AssertUnwindSafe, time::Duration};

#[test]
fn private_files_boot_curated_metadata_and_join_sigterm_without_private_logs() {
    let directory = tempfile::Builder::new()
        .prefix("owned-private-premiumize-runtime-")
        .tempdir()
        .unwrap_or_else(|_| panic!("the owned runtime directory must create"));
    let inputs = Inputs::create(directory.path());
    let mut host = process::Host::start(directory.path(), &inputs);
    let address = host.address;
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        host.wait_ready();
        metadata(address, &inputs)
    }));
    let stopped = host.terminate_and_join();
    inputs.assert_preserved(directory.path());
    let receipts = match outcome {
        Ok(receipts) => receipts,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(stopped.signalled, "the owned runtime must receive SIGTERM");
    assert!(
        stopped.status.success(),
        "the owned runtime must join SIGTERM successfully"
    );
    assert!(
        TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err(),
        "the joined runtime must retire its listener"
    );
    assert!(
        stopped.stderr.is_empty(),
        "runtime diagnostics must stay in structured safe logs"
    );
    logging::assert_metadata_lifecycle(&stopped.stdout, directory.path(), &receipts);
    println!(
        "Premiumize runtime metadata: 6 HTTP requests, 3 auth rejections, 9 correlated events, SIGTERM joined, private inputs preserved"
    );
}

fn metadata(address: std::net::SocketAddr, inputs: &Inputs) -> Vec<http::Receipt> {
    let mut receipts = Vec::new();
    let reply = http::request(address, "GET", "/v1/manifest", Some(SERVICE_TOKEN), "");
    assert_eq!(reply.status, 200);
    receipts.push(reply.receipt("manifest"));
    let manifest: Manifest = serde_json::from_slice(&reply.body)
        .unwrap_or_else(|_| panic!("manifest must be typed SDK JSON"));
    assert!(manifest.validate().is_ok(), "manifest must satisfy the SDK");
    assert!(manifest.id == "org.hito.premiumize");
    assert!(manifest.authorization.is_none());
    assert!(
        manifest.capabilities
            == [
                Capability::Search,
                Capability::Sources,
                Capability::SourcePreparation
            ]
            .into()
    );
    assert!(
        manifest
            .content_policy
            .map(|policy| policy.source_persistence)
            == Some(SourcePersistence::PlaybackOnly)
    );

    let search = json!({"query":TITLE,"cursor":null,"limit":1}).to_string();
    for token in [None, Some(WRONG_TOKEN), Some(PROVIDER_TOKEN)] {
        let reply = http::request(address, "POST", "/v1/search", token, &search);
        assert_eq!(reply.status, 401);
        receipts.push(reply.receipt("search"));
        let error: Value = serde_json::from_slice(&reply.body)
            .unwrap_or_else(|_| panic!("authorization rejection must be JSON"));
        assert!(
            error == json!({"error":"Plugin authorization is required"}),
            "authorization rejection must contain only the safe SDK error"
        );
    }
    let reply = http::request(
        address,
        "POST",
        "/v1/search?owned-private-premiumize-runtime-query=owned-private-premiumize-runtime-value",
        Some(SERVICE_TOKEN),
        &search,
    );
    assert_eq!(reply.status, 200);
    receipts.push(reply.receipt("search"));
    let page: SearchPage = serde_json::from_slice(&reply.body)
        .unwrap_or_else(|_| panic!("search must return typed SDK JSON"));
    assert!(page.validate(1).is_ok(), "search must satisfy the SDK");
    assert_eq!(page.items.len(), 1);
    assert!(
        page.items[0] == inputs.recording,
        "search must return the private configured recording"
    );
    assert!(page.next_cursor.is_none());

    let reply = http::request(
        address,
        "POST",
        "/v1/recordings/detail",
        Some(SERVICE_TOKEN),
        &json!({"recording_id":RECORDING}).to_string(),
    );
    assert_eq!(reply.status, 200);
    receipts.push(reply.receipt("detail"));
    let recording: ProviderRecording = serde_json::from_slice(&reply.body)
        .unwrap_or_else(|_| panic!("detail must return typed SDK JSON"));
    assert!(recording.validate().is_ok(), "detail must satisfy the SDK");
    assert!(
        recording == inputs.recording,
        "detail must preserve the private configured recording"
    );
    receipts
}

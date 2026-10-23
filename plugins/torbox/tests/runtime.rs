#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[path = "runtime_support/http.rs"]
mod http;
#[path = "runtime_support/inputs.rs"]
mod inputs;
#[path = "runtime_support/logging.rs"]
mod logging;
#[allow(dead_code)]
#[path = "runtime_support/process.rs"]
mod process;

use hito_plugin_sdk::{Capability, Manifest, ProviderRecording, SearchPage, SourcePersistence};
use inputs::{Inputs, PROVIDER_TOKEN, RECORDING, SERVICE_TOKEN, TITLE, WRONG_TOKEN};
use serde_json::{Value, json};
use std::{
    net::{SocketAddr, TcpStream},
    panic::AssertUnwindSafe,
    time::{Duration, Instant},
};

#[test]
fn private_files_select_state_modes_cancel_active_ingress_and_reopen() {
    let started = Instant::now();
    let directory = tempfile::Builder::new()
        .prefix("owned-private-torbox-runtime-")
        .tempdir()
        .expect("the owned runtime directory must create");
    let inputs = Inputs::create(directory.path());
    let missing = process::Host::start(directory.path(), &inputs, "open-existing");
    let missing_address = missing.address;
    let rejected = missing.rejected_and_join();
    assert_eq!(rejected.status.code(), Some(1));
    assert!(!rejected.signalled && rejected.stderr.is_empty());
    assert!(
        !inputs.state.exists(),
        "OpenExisting must not create missing state"
    );
    assert_listener_closed(missing_address);
    inputs.assert_preserved(directory.path());
    let rejected_owner = logging::assert_missing_state(&rejected.stdout, directory.path());

    let mut first = process::Host::start(directory.path(), &inputs, "create-new");
    let first_address = first.address;
    let first_outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        first.wait_ready();
        let receipts = metadata(first_address, &inputs);
        let held = http::hold_search_body(first_address, SERVICE_TOKEN);
        (receipts, held)
    }));
    let first_stopped = first.terminate_and_join();
    inputs.assert_preserved(directory.path());
    let (first_receipts, mut held) =
        first_outcome.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert_joined(&first_stopped, first_address);
    http::assert_closed(&mut held);
    let first_owner = logging::assert_lifecycle(
        &first_stopped.stdout,
        directory.path(),
        &first_receipts,
        true,
    );
    let original_database = inputs.database_identity();

    let mut replacement = process::Host::start(directory.path(), &inputs, "open-existing");
    let replacement_address = replacement.address;
    let replacement_outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        replacement.wait_ready();
        metadata(replacement_address, &inputs)
    }));
    let replacement_stopped = replacement.terminate_and_join();
    inputs.assert_preserved(directory.path());
    let replacement_receipts =
        replacement_outcome.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert_joined(&replacement_stopped, replacement_address);
    assert_eq!(inputs.database_identity(), original_database);
    let replacement_owner = logging::assert_lifecycle(
        &replacement_stopped.stdout,
        directory.path(),
        &replacement_receipts,
        false,
    );
    assert!(
        rejected_owner != first_owner
            && first_owner != replacement_owner
            && rejected_owner != replacement_owner
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the owned executable workflow must remain bounded"
    );
    eprintln!(
        "torbox_runtime_evidence children=3 metadata_responses=12 auth_rejections=6 active_request_cancellations=1 joined_owners=2 explicit_missing_state_rejections=1"
    );
}

fn metadata(address: SocketAddr, inputs: &Inputs) -> Vec<http::Receipt> {
    let mut receipts = Vec::new();
    let reply = http::request(address, "GET", "/v1/manifest", None, "");
    assert_eq!(reply.status, 200);
    receipts.push(reply.receipt("manifest"));
    let manifest: Manifest =
        serde_json::from_slice(&reply.body).expect("manifest must use SDK JSON");
    assert!(manifest.validate().is_ok());
    assert!(manifest.id == "org.hito.torbox" && manifest.authorization.is_none());
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
        let body: Value =
            serde_json::from_slice(&reply.body).expect("authorization denial must be JSON");
        assert!(body == json!({"error":"Plugin authorization is required"}));
    }
    let reply = http::request(
        address,
        "POST",
        "/v1/search?owned-private-torbox-runtime-query=owned-private-torbox-runtime-value",
        Some(SERVICE_TOKEN),
        &search,
    );
    assert_eq!(reply.status, 200);
    receipts.push(reply.receipt("search"));
    let page: SearchPage = serde_json::from_slice(&reply.body).expect("search must use SDK JSON");
    assert!(page.validate(1).is_ok());
    assert!(page.items == [inputs.recording.clone()] && page.next_cursor.is_none());
    let reply = http::request(
        address,
        "POST",
        "/v1/recordings/detail",
        Some(SERVICE_TOKEN),
        &json!({"recording_id":RECORDING}).to_string(),
    );
    assert_eq!(reply.status, 200);
    receipts.push(reply.receipt("detail"));
    let recording: ProviderRecording =
        serde_json::from_slice(&reply.body).expect("detail must use SDK JSON");
    assert!(recording.validate().is_ok() && recording == inputs.recording);
    receipts
}

fn assert_joined(stopped: &process::Stopped, address: SocketAddr) {
    assert!(stopped.signalled, "the owned runtime must receive SIGTERM");
    assert!(
        stopped.status.success(),
        "the owned runtime must join SIGTERM successfully"
    );
    assert!(
        stopped.stderr.is_empty(),
        "runtime diagnostics must use safe JSON logs"
    );
    assert_listener_closed(address);
}

fn assert_listener_closed(address: SocketAddr) {
    assert!(
        TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err(),
        "the joined runtime must retire its listener"
    );
}

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
use process::Host;
use serde_json::{Value, json};
use std::{net::TcpStream, time::Duration};

#[test]
fn private_inputs_boot_the_authenticated_metadata_service_and_join_sigterm() {
    let directory = tempfile::tempdir().expect("owned runtime directory must create");
    let inputs = Inputs::create(directory.path());
    let mut host = Host::start(directory.path(), &inputs);
    host.wait_ready();

    let (status, bytes) = http::request(host.address, "GET", "/v1/manifest", None, "");
    assert_eq!(status, 200);
    let manifest: Manifest = serde_json::from_slice(&bytes).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.id, "org.hito.all-debrid");
    assert!(
        manifest.capabilities
            == [
                Capability::Search,
                Capability::Sources,
                Capability::SourcePreparation
            ]
            .into()
    );
    assert_eq!(
        manifest.content_policy.unwrap().source_persistence,
        SourcePersistence::PlaybackOnly
    );

    let search = json!({"query": TITLE, "cursor": null, "limit": 1}).to_string();
    for credential in [None, Some(WRONG_TOKEN), Some(PROVIDER_TOKEN)] {
        let (status, bytes) =
            http::request(host.address, "POST", "/v1/search", credential, &search);
        assert_eq!(status, 401);
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({"error":"Plugin authorization is required"})
        );
    }
    let (status, bytes) = http::request(
        host.address,
        "POST",
        "/v1/search?owned-private-bootstrap-query=owned-private-bootstrap-value",
        Some(SERVICE_TOKEN),
        &search,
    );
    assert_eq!(status, 200);
    let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
    page.validate(1).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        serde_json::to_value(&page.items[0]).unwrap(),
        inputs.recording
    );
    assert!(page.next_cursor.is_none());

    let (status, bytes) = http::request(
        host.address,
        "POST",
        "/v1/recordings/detail",
        Some(SERVICE_TOKEN),
        &json!({"recording_id":RECORDING}).to_string(),
    );
    assert_eq!(status, 200);
    let recording: ProviderRecording = serde_json::from_slice(&bytes).unwrap();
    recording.validate().unwrap();
    assert_eq!(serde_json::to_value(recording).unwrap(), inputs.recording);

    host.signal_term();
    assert!(
        host.wait_exit().success(),
        "owned runtime must join SIGTERM successfully"
    );
    assert!(
        TcpStream::connect_timeout(&host.address, Duration::from_millis(100)).is_err(),
        "the joined runtime must retire its listener"
    );
    inputs.assert_preserved(directory.path());
    logging::assert_metadata_lifecycle(&host.logs(), directory.path());
}

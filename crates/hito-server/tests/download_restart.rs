#[path = "../../hito-core/tests/fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod fixture;
#[path = "fixtures/download_host.rs"]
mod host;
#[path = "fixtures/artwork_http.rs"]
mod http;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use audio_fixture::AudioLibrary;
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::DownloadState;
use host::Host;
use http::{call, get};
use serde_json::json;
use std::{net::SocketAddr, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

const HOST: &str = "provider-audio";

#[tokio::test]
async fn an_unfinished_download_requeues_on_joined_shutdown_and_completes_after_restart() {
    let library = AudioLibrary::start().await;
    library.ready().await;
    let gate = library.server.hold_broken_tail(64).await;
    let AudioLibrary {
        directory,
        music,
        server,
        hito,
        source,
        original: audio,
        ..
    } = library;
    let original = support::write_wav(&music.join("original.wav"), 23);
    let mut hosts = vec![Host::start(hito).await];
    let outcome = AssertUnwindSafe(exercise(
        directory.path(),
        &server,
        &gate,
        &source.recording_id,
        &audio,
        &mut hosts,
    ))
    .catch_unwind()
    .await;
    for host in &mut hosts {
        host.request_shutdown();
    }
    gate.open();
    let mut joined = true;
    for host in &mut hosts {
        joined &= host.join().await;
    }
    assert!(
        joined,
        "all owned host workers must retire before fixture teardown"
    );
    assert!(
        std::fs::read(music.join("original.wav")).unwrap() == original,
        "interrupted downloads must preserve user-owned original bytes"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(
    directory: &Path,
    provider: &fixture::ProviderServer,
    gate: &fixture::Gate,
    recording: &str,
    audio: &[u8],
    hosts: &mut Vec<Host>,
) {
    let address = hosts[0].address;
    let response = call(
        address,
        "POST",
        "/api/v1/login",
        Some(json!({"username": "admin", "password": "indexing test password"})),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    let session = response.json();
    let token = session["token"].as_str().unwrap();
    save_private_state(address, token, recording).await;
    let metadata_path = format!("/api/v1/recordings/{recording}/metadata");
    let metadata = get(address, token, &metadata_path).await;
    assert!(metadata["title"]["personal"] == "My retained restart title");
    let status_path = format!("/api/v1/recordings/{recording}/download");
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    let request = json!({"request_id": Uuid::new_v4()});
    assert_eq!(provider.requests("v1/audio").await.len(), 0);
    let response = call(
        address,
        "POST",
        &status_path,
        Some(request.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let receipt = response.json();
    let intent = receipt["intent_id"].as_str().unwrap();
    assert!(receipt["request_id"] == request["request_id"]);
    assert!(receipt["recording_id"] == recording);
    tokio::time::timeout(Duration::from_secs(4), gate.entered())
        .await
        .expect("the owned provider must hold the first body after 64 bytes");
    let running = get(address, token, &status_path).await;
    assert!(running["status"] == "running");
    assert!(running["source_id"].is_null());
    assert_eq!(provider.requests("v1/audio").await.len(), 1);
    let sources = get(address, token, &sources_path).await;
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert!(sources[0]["provider"] == "plugin");

    hosts[0].request_shutdown();
    assert!(
        hosts[0].join().await,
        "shutdown must join the unfinished transfer before its body gate opens"
    );
    assert_eq!(
        std::fs::read_dir(directory.join("hito.downloads"))
            .unwrap()
            .count(),
        0,
        "the retired incomplete attempt must leave no final or partial file"
    );
    assert_eq!(provider.requests("v1/audio").await.len(), 1);
    let reopened = host::open(directory, HOST).await;
    let actor = reopened.identity.authenticate(token).await.unwrap();
    let queued = reopened.downloads.status(&actor, recording).await.unwrap();
    assert!(matches!(queued.status, DownloadState::Queued));
    assert!(queued.intent_id == intent);
    assert!(queued.recording_id == recording);
    assert!(queued.source_id.is_none());
    assert!(queued.error.is_none());
    provider.clear_audio_tail_fault().await;
    provider
        .audio(StatusCode::OK, audio_fixture::headers(), audio.to_vec())
        .await;
    hosts.push(Host::start(reopened).await);
    let address = hosts[1].address;
    let ready = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let status = get(address, token, &status_path).await;
            if status["status"] == "ready" {
                break status;
            }
            assert!(matches!(
                status["status"].as_str(),
                Some("queued" | "running" | "verifying")
            ));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the retained intent must finish without another Download action");
    assert!(ready["intent_id"] == intent);
    assert!(ready["recording_id"] == recording);
    assert!(ready["host_device_id"] == HOST);
    assert!(ready["bytes_received"] == audio.len());
    let source = ready["source_id"].as_str().unwrap();
    let response = call(
        address,
        "GET",
        &format!("/api/v1/sources/{source}/audio"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["content-type"], "audio/wav");
    assert_eq!(response.headers["content-length"], audio.len().to_string());
    assert_eq!(response.body.len(), audio.len());
    assert!(
        response.body == audio,
        "only the complete second transfer may become playable"
    );
    assert_eq!(provider.requests("v1/audio").await.len(), 2);
    let response = call(address, "POST", &status_path, Some(request), Some(token)).await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert!(
        response.json() == receipt,
        "reopening must preserve the original request receipt"
    );
    assert_eq!(provider.requests("v1/audio").await.len(), 2);
    assert!(get(address, token, &metadata_path).await == metadata);
    let catalog = get(address, token, "/api/v1/recordings?liked=true").await;
    assert_eq!(catalog["items"].as_array().unwrap().len(), 1);
    assert!(catalog["items"][0]["id"] == recording);
    let activity = get(address, token, "/api/v1/activity").await;
    assert_eq!(activity["history"].as_array().unwrap().len(), 1);
    assert!(activity["history"][0]["id"] == recording);
    assert!(activity["history"][0]["position_ms"] == 700);
    assert!(activity["history"][0]["speed"] == 1.25);
    hosts[1].request_shutdown();
    assert!(hosts[1].join().await);
    let files = std::fs::read_dir(directory.join("hito.downloads"))
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        files.len(),
        1,
        "only the retained verified file may remain after workers retire"
    );
    assert!(files[0].file_type().unwrap().is_file());
    assert!(
        files[0]
            .path()
            .extension()
            .is_some_and(|extension| extension == "media")
    );
    assert!(std::fs::read(files[0].path()).unwrap() == audio);
}

async fn save_private_state(address: SocketAddr, token: &str, recording: &str) {
    let operations = [
        json!({"kind": "recording_title_set", "recording_id": recording, "title": "My retained restart title"}),
        json!({"kind": "like_set", "recording_id": recording, "liked": true}),
        json!({"kind": "progress_set", "recording_id": recording, "position_ms": 700, "speed": 1.25}),
    ].into_iter().map(|action| json!({"id": Uuid::new_v4(), "action": action})).collect::<Vec<_>>();
    let response = call(
        address,
        "POST",
        "/api/v1/sync",
        Some(json!({"device_id": "download-restart-client", "operations": operations})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json()["acknowledged"].as_array().unwrap().len(), 3);
}

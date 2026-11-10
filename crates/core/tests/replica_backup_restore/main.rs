#![cfg(target_os = "linux")]

mod archive;
mod owner;
#[allow(dead_code)]
#[path = "../replicas/fixture.rs"]
mod remote;

use hito_core::{Action, SyncEvent, SyncPage};
use std::{path::Path, process::Stdio, time::Duration};

const TEST: &str = "stopped_complete_data_archive_restores_standalone_and_replica_outbox";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopped_complete_data_archive_restores_standalone_and_replica_outbox() {
    if let Ok(stage) = std::env::var("HITO_REPLICA_BACKUP_STAGE") {
        owner::run(&stage).await;
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let source = root.join("source-data");
    let restored = root.join("restored-data");
    let music = root.join("original-media");
    for path in [&source, &restored, &music] {
        archive::private_directory(path);
    }
    let original = music.join("owned-original.wav");
    archive::private_file(&original, &[]);
    hito_core::test_support::support::write_wav(&original, 37);
    let original_before = archive::snapshot(&music);
    let manifest = root.join("expectations.json");
    let remote = remote::RemoteServer::start().await;
    run_owner("seed", &source, &music, &manifest, &remote.endpoint).await;
    let proof: owner::Proof = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(remote.event_count(), 1);
    let before = history(&remote.endpoint).await;
    assert_eq!(before.operations.len(), 1);
    assert_event(&before.operations[0], &proof, false);
    archive::assert_private_data(&source, proof.connection);
    let source_before = archive::snapshot(&source);
    let tar = root.join("complete-stopped-data.tar");
    archive::create(&source, &tar).await;
    let archive_before = std::fs::read(&tar).unwrap();
    archive::extract(&tar, &restored).await;
    assert!(
        source_before == archive::snapshot(&restored),
        "complete stopped archive changed file bytes, modes, or ownership"
    );
    archive::assert_private_data(&restored, proof.connection);
    run_owner("restored", &restored, &music, &manifest, &remote.endpoint).await;
    assert_eq!(remote.event_count(), 2);
    let after = history(&remote.endpoint).await;
    assert_eq!(after.operations.len(), 2);
    assert_eq!(after.cursor, 2);
    assert_event(&after.operations[0], &proof, false);
    assert_event(&after.operations[1], &proof, true);
    assert!(
        source_before == archive::snapshot(&source),
        "restoration modified the stopped source data"
    );
    assert!(
        original_before == archive::snapshot(&music),
        "restoration modified user-owned original media"
    );
    assert!(
        archive_before == std::fs::read(&tar).unwrap(),
        "restoration modified the preserved archive"
    );
    archive::assert_private_data(&restored, proof.connection);
    eprintln!(
        "Stopped Linux replica backup acceptance: complete archive restored; accepted_events=1 pending_events=1 final_events=2 source_and_original_preserved=true"
    );
}

async fn run_owner(stage: &str, data: &Path, music: &Path, manifest: &Path, endpoint: &str) {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env("HITO_REPLICA_BACKUP_STAGE", stage)
        .env("HITO_REPLICA_BACKUP_DATA", data)
        .env("HITO_REPLICA_BACKUP_MUSIC", music)
        .env("HITO_REPLICA_BACKUP_MANIFEST", manifest)
        .env("HITO_REPLICA_BACKUP_ENDPOINT", endpoint)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .expect("owned Core process did not stop within its fuse")
        .unwrap();
    assert!(
        output.status.success(),
        "owned Core {stage} process failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn history(endpoint: &str) -> SyncPage {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let response = client
        .get(format!("{endpoint}/api/v1/sync?cursor=0"))
        .bearer_auth(remote::TOKEN)
        .header("x-hito-account", remote::REMOTE_ACCOUNT)
        .send()
        .await;
    let mut response = match response {
        Ok(response) => response,
        Err(_) => panic!("owned remote history is unavailable"),
    };
    assert_eq!(response.status().as_u16(), 200);
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.expect("owned history body failed") {
        assert!(chunk.len() <= 65_536_usize.saturating_sub(body.len()));
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .expect("owned history did not use the actual synchronization wire type")
}

fn assert_event(event: &SyncEvent, proof: &owner::Proof, pending: bool) {
    let SyncEvent::Operation {
        sequence,
        device_id,
        operation,
    } = event
    else {
        panic!("empty-playlist acceptance unexpectedly received a publication")
    };
    assert_eq!(*sequence, if pending { 2 } else { 1 });
    assert_eq!(device_id, &proof.client);
    assert_eq!(
        operation.id,
        if pending {
            proof.pending
        } else {
            proof.accepted
        }
    );
    match (&operation.action, pending) {
        (Action::PlaylistCreate { playlist_id, name }, false) => {
            assert!(*playlist_id == proof.remote_playlist && name == owner::BASE_NAME)
        }
        (Action::PlaylistRename { playlist_id, name }, true) => {
            assert!(*playlist_id == proof.remote_playlist && name == owner::OFFLINE_NAME)
        }
        _ => panic!("restored history changed an owned operation payload"),
    }
}

use super::{archive, catalog_fixture, catalog_owner, remote};
use std::{path::Path, process::Stdio, time::Duration};
use uuid::Uuid;

const TEST: &str = "catalog::stopped_complete_archive_restores_populated_catalog_offline";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopped_complete_archive_restores_populated_catalog_offline() {
    if let Ok(stage) = std::env::var("HITO_CATALOG_BACKUP_STAGE") {
        Box::pin(catalog_owner::run(&stage)).await;
        return;
    }
    let started = std::time::Instant::now();
    let correlation = Uuid::new_v4();
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
    hito_core::test_support::support::write_wav(&original, 43);
    let original_before = archive::snapshot(&music);
    let remote = remote::RemoteServer::start().await;
    let mut gate = catalog_fixture::PeerGate::start(&remote.endpoint).await;
    let snapshot = Uuid::new_v4();
    let (metadata, pages) = catalog_fixture::catalog(snapshot);
    remote.catalog(
        serde_json::to_value(metadata).unwrap(),
        pages
            .into_iter()
            .map(|page| serde_json::to_value(page).unwrap())
            .collect(),
    );
    finish(owner("seed", root, &source, &gate.endpoint(), snapshot)).await;
    let proof: catalog_owner::Proof =
        serde_json::from_slice(&std::fs::read(root.join("catalog-expectations.json")).unwrap())
            .unwrap();
    let before = super::history(&remote.endpoint).await;
    assert_eq!(before.operations.len(), 1);
    assert_eq!(before.cursor, 1);
    super::assert_event(&before.operations[0], &proof.replica, false);
    assert_eq!(remote.event_count(), 1);
    gate.stop().await;
    archive::assert_private_data(&source, proof.replica.connection);
    let source_before = archive::snapshot(&source);
    let tar = root.join("complete-populated-data.tar");
    archive::create(&source, &tar).await;
    let archive_before = std::fs::read(&tar).unwrap();
    archive::extract(&tar, &restored).await;
    assert!(
        source_before == archive::snapshot(&restored),
        "restored files changed bytes, modes or ownership before reopen"
    );
    archive::assert_private_data(&restored, proof.replica.connection);
    let child = owner("restored", root, &restored, &gate.endpoint(), snapshot);
    let mut completed = tokio::spawn(finish(child));
    let offline_verified = root.join("offline-verified");
    tokio::select! {
        () = catalog_fixture::wait_for_signal(&offline_verified) => {}
        result = &mut completed => {
            result.unwrap();
            panic!("restored owner exited before proving offline reads");
        }
    }
    assert_eq!(
        remote.event_count(),
        1,
        "offline reads submitted collection operations"
    );
    gate.resume().await;
    archive::private_file(&root.join("peer-resumed"), &[]);
    completed.await.unwrap();
    let after = super::history(&remote.endpoint).await;
    assert_eq!(remote.event_count(), 2);
    assert_eq!(after.operations.len(), 2);
    assert_eq!(after.cursor, 2);
    super::assert_event(&after.operations[0], &proof.replica, false);
    super::assert_event(&after.operations[1], &proof.replica, true);
    gate.stop().await;
    assert!(
        source_before == archive::snapshot(&source),
        "clone use changed the stopped source"
    );
    assert!(
        original_before == archive::snapshot(&music),
        "clone use changed original audio"
    );
    assert!(
        archive_before == std::fs::read(&tar).unwrap(),
        "clone use changed the archive"
    );
    archive::assert_private_data(&restored, proof.replica.connection);
    eprintln!(
        "Stopped Linux populated-catalog backup acceptance: correlation_id={correlation} pages=2 recordings=2 accepted_events=1 pending_events=1 final_events=2 source_archive_original_preserved=true elapsed_ms={}",
        started.elapsed().as_millis()
    );
}

fn owner(
    stage: &str,
    root: &Path,
    data: &Path,
    endpoint: &str,
    snapshot: Uuid,
) -> tokio::process::Child {
    tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env("HITO_CATALOG_BACKUP_STAGE", stage)
        .env("HITO_CATALOG_BACKUP_ROOT", root)
        .env("HITO_CATALOG_BACKUP_DATA", data)
        .env("HITO_CATALOG_BACKUP_ENDPOINT", endpoint)
        .env("HITO_CATALOG_BACKUP_SNAPSHOT", snapshot.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

async fn finish(child: tokio::process::Child) {
    let result = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .expect("owned catalog Core process did not stop within its fuse")
        .unwrap();
    assert!(
        result.status.success(),
        "owned catalog Core process failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#![cfg(unix)]
#[path = "fixtures/legacy_library.rs"]
#[allow(dead_code)]
mod legacy;
#[path = "fixtures/media_tool.rs"]
mod media_tool;
mod support;

use hito_core::{CoreConfig, HostDeviceId, Search};
use std::time::Duration;

#[tokio::test]
async fn removing_a_legacy_witness_stops_its_decoder_during_another_folders_scan() {
    use hito_core::{ConnectFolder, DirectoryRef};
    let fixture = legacy::Legacy::seed().await;
    let marker = fixture.directory.path().join("witness-decoder");
    let config = CoreConfig::new(
        fixture.directory.path().join("hito.db"),
        fixture.music.clone(),
        HostDeviceId::new("server").unwrap(),
    );
    let services = hito_core::open(config).await.unwrap();
    let actor = services
        .identity
        .authenticate("legacy-fixture-token")
        .await
        .unwrap();
    let witness = services
        .storage
        .folders(&actor, legacy::LIBRARY)
        .await
        .unwrap()
        .remove(0);
    let directory = fixture.directory.path().join("other");
    std::fs::create_dir(&directory).unwrap();
    drop(services);
    let mut config = CoreConfig::with_shared_directories(
        fixture.directory.path().join("hito.db"),
        HostDeviceId::new("server").unwrap(),
        vec![
            hito_core::SharedDirectoryConfig {
                id: "legacy-music".into(),
                name: "Music".into(),
                root: fixture.music.clone(),
            },
            hito_core::SharedDirectoryConfig {
                id: "other".into(),
                name: "Other".into(),
                root: directory,
            },
        ],
    );
    config.media_tools.ffmpeg = media_tool::script(
        fixture.directory.path(),
        "ffmpeg",
        &format!(
            "printf '%s\\n' \"$$\" > {}\nexec sleep 300",
            media_tool::quote(&marker)
        ),
    );
    let services = hito_core::open(config).await.unwrap();
    let target = services
        .storage
        .connect_folder(
            &actor,
            legacy::LIBRARY,
            ConnectFolder {
                directory: DirectoryRef {
                    directory_id: "other".into(),
                    components: Vec::new(),
                },
                name: None,
            },
        )
        .await
        .unwrap();
    let storage = services.storage.clone();
    let worker = tokio::spawn(async move { storage.run_next_job().await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    services
        .storage
        .remove_folder(&actor, &witness.id, witness.revision)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .expect("Removing a witness must stop its inspection promptly")
            .unwrap()
            .unwrap()
    );
    let current = services
        .storage
        .folders(&actor, legacy::LIBRARY)
        .await
        .unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].id, target.id);
    assert_eq!(current[0].latest_job.as_ref().unwrap().state, "complete");
    let pid = std::fs::read_to_string(marker).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The removed witness's decoder must be reaped");
}

#[tokio::test]
async fn removing_a_folder_stops_its_running_decoder_and_prevents_late_publication() {
    let (dir, original, actor, library, music) = support::library("server").await;
    let path = music.join("track.wav");
    support::write_wav(&path, 0);
    support::scan(&original, &actor, &library).await;
    let folder = original
        .storage
        .folders(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    let marker = dir.path().join("decoder");
    let decoder = media_tool::script(
        dir.path(),
        "ffmpeg",
        &format!(
            "printf '%s\\n' \"$$\" > {}\nexec sleep 300",
            media_tool::quote(&marker),
        ),
    );
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffmpeg = decoder;
    let services = hito_core::open(config).await.unwrap();
    support::write_wav(&path, 1);
    let job = services
        .storage
        .enqueue_folder_scan(&actor, &folder.id)
        .await
        .unwrap();
    let storage = services.storage.clone();
    let worker = tokio::spawn(async move { storage.run_next_job().await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    services
        .storage
        .remove_folder(&actor, &folder.id, folder.revision)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    assert_eq!(
        services.storage.job(&actor, &job.id).await.unwrap().state,
        "failed"
    );
    let recordings = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert!(!recordings.items[0].available);
    let pid = std::fs::read_to_string(marker).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !std::process::Command::new("kill")
                .args(["-0", pid.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The detached folder's decoder must be reaped");
    assert!(path.exists());
}

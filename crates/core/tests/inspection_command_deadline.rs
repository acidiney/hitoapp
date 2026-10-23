#![cfg(unix)]

#[path = "fixtures/inspection_command_tools.rs"]
mod command_tools;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    ConnectFolder, CoreConfig, DirectoryRef, HostDeviceId, LibraryStorage, Search,
    SharedDirectoryConfig,
};
use std::{
    panic::AssertUnwindSafe,
    time::{Duration, Instant},
};
use tokio::{sync::watch, task::JoinHandle};

const COMMAND_BUDGET: Duration = Duration::from_secs(4);

#[tokio::test]
async fn media_inspection_keeps_temporary_audio_private_until_its_decoder_retires() {
    let (directory, _original, actor, library, music) = support::library("snapshot-privacy").await;
    let path = music.join("track.wav");
    let original = support::write_wav(&path, 0);
    let tools = command_tools::Tools::new(directory.path());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("snapshot-privacy").unwrap(),
    );
    config.media_tools.ffprobe = tools.ffprobe.clone();
    config.media_tools.ffmpeg = tools.ffmpeg.clone();
    config.media_tools.timeout = COMMAND_BUDGET;
    let services = hito_core::open(config).await.unwrap();
    services
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap();
    let (stop, _) = watch::channel(false);
    let worker = run(&services.storage, &stop);
    let outcome = AssertUnwindSafe(async {
        let decoder = tools.decoders(1).await.remove(0);
        decoder.assert_private_input();
        assert_eq!(std::fs::read(&path).unwrap(), original);
    })
    .catch_unwind()
    .await;

    stop.send_replace(true);
    let joined = worker.await;
    let settled = services.storage.finish_indexing().await;
    let retired = tools.retired().await;
    assert!(joined.unwrap().unwrap());
    settled.unwrap();
    assert!(
        retired,
        "The configured decoder and its private input must retire after cancellation"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[tokio::test]
async fn a_media_command_deadline_includes_waiting_for_decoder_capacity() {
    let (directory, _original, actor, library, music) = support::library("command-deadline").await;
    let mut roots = vec![music];
    for name in ["second", "third"] {
        let root = directory.path().join(name);
        std::fs::create_dir(&root).unwrap();
        roots.push(root);
    }
    let originals: Vec<_> = roots
        .iter()
        .enumerate()
        .map(|(offset, root)| {
            let path = root.join("track.wav");
            let bytes = support::write_wav(&path, offset as i16);
            (path, bytes)
        })
        .collect();
    let tools = command_tools::Tools::new(directory.path());
    let mut config = CoreConfig::with_shared_directories(
        directory.path().join("hito.db"),
        HostDeviceId::new("command-deadline").unwrap(),
        roots
            .into_iter()
            .enumerate()
            .map(|(index, root)| SharedDirectoryConfig {
                id: format!("folder-{index}"),
                name: format!("Folder {index}"),
                root,
            })
            .collect(),
    );
    config.media_tools.ffprobe = tools.ffprobe.clone();
    config.media_tools.ffmpeg = tools.ffmpeg.clone();
    config.media_tools.timeout = COMMAND_BUDGET;
    let services = hito_core::open(config).await.unwrap();
    let mut folders = Vec::new();
    for index in 0..3 {
        folders.push(
            services
                .storage
                .connect_folder(
                    &actor,
                    &library,
                    ConnectFolder {
                        directory: DirectoryRef {
                            directory_id: format!("folder-{index}"),
                            components: Vec::new(),
                        },
                        name: None,
                    },
                )
                .await
                .unwrap(),
        );
    }
    let third_job = folders[2].latest_job.as_ref().unwrap().id.clone();
    let (stop, _) = watch::channel(false);
    let mut workers = Vec::new();
    let outcome = AssertUnwindSafe(async {
        workers.push(run(&services.storage, &stop));
        tools.decoders(1).await;
        let first_decoder = Instant::now();
        workers.push(run(&services.storage, &stop));
        let decoders = tools.decoders(2).await;
        assert!(
            first_decoder.elapsed() < Duration::from_secs(1),
            "Both decoder slots must be occupied well before their deadlines"
        );
        for process in &decoders {
            process.assert_live_input();
        }
        workers.push(run(&services.storage, &stop));
        let completed = tokio::time::timeout(COMMAND_BUDGET + Duration::from_millis(1500), async {
            loop {
                let job = services.storage.job(&actor, &third_job).await.unwrap();
                if !matches!(job.state.as_str(), "queued" | "running") {
                    return job;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        if completed.is_err() {
            assert!(!workers[2].is_finished());
            assert_eq!(
                services
                    .storage
                    .job(&actor, &third_job)
                    .await
                    .unwrap()
                    .state,
                "running"
            );
        }
        assert!(
            completed.is_ok(),
            "One media command exceeded its deadline while waiting for decoder capacity"
        );
        assert_eq!(completed.unwrap().state, "failed");
    })
    .catch_unwind()
    .await;

    stop.send_replace(true);
    let mut joins = Box::pin(futures_util::future::join_all(workers));
    let timely_cleanup = tokio::time::timeout(Duration::from_secs(6), &mut joins).await;
    let joined = match timely_cleanup {
        Ok(joined) => joined,
        Err(_) => {
            tools.kill_recorded();
            tokio::time::timeout(Duration::from_secs(6), &mut joins)
                .await
                .unwrap()
        }
    };
    let settled = services.storage.finish_indexing().await;
    let retired = tools.retired().await;
    for result in joined {
        assert!(result.unwrap().unwrap());
    }
    settled.unwrap();
    assert!(
        retired,
        "All configured media processes and private inputs must retire"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }

    for folder in folders.iter().take(2) {
        services
            .storage
            .remove_folder(&actor, &folder.id, folder.revision)
            .await
            .unwrap();
    }
    tools.release();
    let job = services
        .storage
        .enqueue_folder_scan(&actor, &folders[2].id)
        .await
        .unwrap();
    assert!(services.storage.run_next_job().await.unwrap());
    assert_eq!(
        services.storage.job(&actor, &job.id).await.unwrap().state,
        "complete"
    );
    let recordings = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert!(recordings.items[0].available);
    for (path, original) in originals {
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}

fn run(
    storage: &LibraryStorage,
    stop: &watch::Sender<bool>,
) -> JoinHandle<hito_core::Result<bool>> {
    let storage = storage.clone();
    let mut stop = stop.subscribe();
    tokio::spawn(async move {
        storage
            .run_next_job_until(async move {
                let stopping = *stop.borrow();
                if !stopping {
                    let _ = stop.changed().await;
                }
            })
            .await
    })
}

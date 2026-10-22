#![cfg(target_os = "linux")]
use hito_core::test_support::support;
use hito_core::{
    ConnectFolder, CoreConfig, Credentials, DirectoryRef, HostDeviceId, Search,
    SharedDirectoryConfig,
};
use std::time::Duration;

#[tokio::test]
#[ignore = "Run with RUSTFLAGS='--cfg rustix_use_libc' for filesystem fault injection"]
async fn stalled_enumeration_preserves_sources_and_releases_the_worker_for_healthy_folders() {
    if std::env::var_os("HITO_WALK_CHILD").is_some() {
        child().await;
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("hito-delayed-walk");
    let healthy = directory.path().join("healthy");
    std::fs::create_dir(&music).unwrap();
    std::fs::create_dir(&healthy).unwrap();
    support::write_wav(&music.join("original.wav"), 0);
    support::write_wav(&healthy.join("second.wav"), 1);
    let services = hito_core::open(CoreConfig::new(
        directory.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new("server").unwrap(),
    ))
    .await
    .unwrap();
    let session = services.identity.setup(credentials()).await.unwrap();
    let actor = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let library = services
        .catalog
        .libraries(&actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    support::scan(&services, &actor, &library).await;
    let source = directory.path().join("filesystem.c");
    let shim = directory.path().join("filesystem.so");
    std::fs::write(&source, r#"
        #define _GNU_SOURCE
        #include <dirent.h>
        #include <dlfcn.h>
        #include <stdio.h>
        #include <stdlib.h>
        #include <string.h>
        #include <time.h>
        #include <unistd.h>
        struct dirent64 *readdir64(DIR *directory) {
            char descriptor[64], path[4096];
            snprintf(descriptor, sizeof descriptor, "/proc/self/fd/%d", dirfd(directory));
            ssize_t size = readlink(descriptor, path, sizeof path - 1);
            if (size > 0) {
                path[size] = 0;
                if (strstr(path, "hito-delayed-walk")) {
                    struct timespec duration = {strcmp(getenv("HITO_WALK_CHILD"), "progress") ? 12 : 1, 0};
                    nanosleep(&duration, 0);
                }
            }
            struct dirent64 *(*original)(DIR *) = dlsym(RTLD_NEXT, "readdir64");
            return original(directory);
        }
    "#).unwrap();
    assert!(
        std::process::Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&shim)
            .arg(&source)
            .arg("-ldl")
            .status()
            .unwrap()
            .success()
    );
    for mode in ["stall", "progress"] {
        for index in 0..6 {
            std::fs::write(music.join(format!("unsupported-{index}.txt")), "fixture").unwrap();
        }
        let mut process = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "stalled_enumeration_preserves_sources_and_releases_the_worker_for_healthy_folders",
            ])
            .env("HITO_WALK_CHILD", mode)
            .env("HITO_WALK_FIXTURE", directory.path())
            .env("LD_PRELOAD", &shim)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let timeout = if mode == "stall" { 8 } else { 18 };
        assert!(
            tokio::time::timeout(Duration::from_secs(timeout), process.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

fn credentials() -> Credentials {
    Credentials {
        username: "admin".into(),
        password: "walking fixture password".into(),
    }
}

async fn child() {
    let root = std::path::PathBuf::from(std::env::var_os("HITO_WALK_FIXTURE").unwrap());
    let services = hito_core::open(CoreConfig::with_shared_directories(
        root.join("hito.db"),
        HostDeviceId::new("server").unwrap(),
        vec![
            SharedDirectoryConfig {
                id: "legacy-music".into(),
                name: "Original".into(),
                root: root.join("hito-delayed-walk"),
            },
            SharedDirectoryConfig {
                id: "healthy".into(),
                name: "Healthy".into(),
                root: root.join("healthy"),
            },
        ],
    ))
    .await
    .unwrap();
    let session = services.identity.login(credentials()).await.unwrap();
    let actor = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let library = services
        .catalog
        .libraries(&actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    let folder = services
        .storage
        .folders(&actor, &library)
        .await
        .unwrap()
        .into_iter()
        .find(|folder| folder.directory.as_ref().unwrap().directory_id == "legacy-music")
        .unwrap();
    let job = services
        .storage
        .enqueue_folder_scan(&actor, &folder.id)
        .await
        .unwrap();
    services
        .storage
        .connect_folder(
            &actor,
            &library,
            ConnectFolder {
                directory: DirectoryRef {
                    directory_id: "healthy".into(),
                    components: Vec::new(),
                },
                name: None,
            },
        )
        .await
        .unwrap();
    let started = std::time::Instant::now();
    assert!(services.storage.run_next_job().await.unwrap());
    let expected = if std::env::var("HITO_WALK_CHILD").unwrap() == "stall" {
        "failed"
    } else {
        assert!(started.elapsed() > Duration::from_secs(5));
        "complete"
    };
    assert_eq!(
        services.storage.job(&actor, &job.id).await.unwrap().state,
        expected
    );
    let existing = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert!(
        existing
            .items
            .iter()
            .any(|recording| recording.title == "original" && recording.available)
    );
    if expected == "failed" {
        assert!(services.storage.run_next_job().await.unwrap());
        let recordings = services
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap();
        assert_eq!(recordings.total, 2);
        assert!(recordings.items.iter().all(|recording| recording.available));
    }
}

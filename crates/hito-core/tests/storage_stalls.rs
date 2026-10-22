#![cfg(target_os = "linux")]
use hito_core::test_support::support;
use hito_core::{BrowseDirectories, CoreConfig, DirectoryRef, HostDeviceId, SharedDirectoryConfig};
use std::time::Duration;

#[tokio::test]
async fn a_stalled_shared_directory_has_a_deadline_and_cannot_block_a_healthy_share() {
    if std::env::var_os("HITO_STORAGE_STALL_CHILD").is_none() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("filesystem.c");
        let library = fixture.path().join("filesystem.so");
        std::fs::write(
            &source,
            r#"
            #define _GNU_SOURCE
            #include <dlfcn.h>
            #include <string.h>
            #include <time.h>
            char *realpath(const char *path, char *resolved) {
                if (strstr(path, "hito-delayed-root")) {
                    struct timespec duration = {12, 0};
                    nanosleep(&duration, 0);
                }
                char *(*original)(const char *, char *) = dlsym(RTLD_NEXT, "realpath");
                return original(path, resolved);
            }
        "#,
        )
        .unwrap();
        assert!(
            std::process::Command::new("cc")
                .args(["-shared", "-fPIC", "-o"])
                .arg(&library)
                .arg(&source)
                .arg("-ldl")
                .status()
                .unwrap()
                .success()
        );
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "a_stalled_shared_directory_has_a_deadline_and_cannot_block_a_healthy_share",
            ])
            .env("HITO_STORAGE_STALL_CHILD", "1")
            .env("LD_PRELOAD", library)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let status = tokio::time::timeout(Duration::from_secs(8), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success());
        return;
    }
    let (dir, _original, actor, _library, music) = support::library("server").await;
    let slow = dir.path().join("hito-delayed-root");
    std::fs::create_dir(&slow).unwrap();
    let services = hito_core::open(CoreConfig::with_shared_directories(
        dir.path().join("hito.db"),
        HostDeviceId::new("server").unwrap(),
        vec![
            SharedDirectoryConfig {
                id: "slow".into(),
                name: "Slow".into(),
                root: slow,
            },
            SharedDirectoryConfig {
                id: "healthy".into(),
                name: "Healthy".into(),
                root: music,
            },
        ],
    ))
    .await
    .unwrap();
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let storage = services.storage.clone();
        let actor = actor.clone();
        requests.spawn(async move {
            storage
                .browse(
                    &actor,
                    BrowseDirectories {
                        directory: DirectoryRef {
                            directory_id: "slow".into(),
                            components: Vec::new(),
                        },
                        offset: None,
                    },
                )
                .await
        });
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let healthy = tokio::time::timeout(
        Duration::from_secs(2),
        services.storage.browse(
            &actor,
            BrowseDirectories {
                directory: DirectoryRef {
                    directory_id: "healthy".into(),
                    components: Vec::new(),
                },
                offset: None,
            },
        ),
    )
    .await;
    assert!(
        healthy
            .expect("A healthy grant must not wait on another grant's filesystem calls")
            .is_ok()
    );
    tokio::time::timeout(Duration::from_secs(6), async {
        while let Some(result) = requests.join_next().await {
            assert!(result.unwrap().is_err());
        }
    })
    .await
    .expect("Stalled directory requests must have a bounded response time");
}

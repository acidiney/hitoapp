use hito_core::test_support::support;

use hito_core::{BrowseDirectories, ConnectFolder, Credentials, Error};

#[tokio::test]
async fn a_host_directory_inside_a_grant_reuses_that_grant() {
    let (_dir, services, admin, _library, music) = support::library("server").await;
    std::fs::create_dir_all(music.join("Jazz").join("Live")).unwrap();
    let reference = services
        .storage
        .grant_host_directory(&admin, &music.join("Jazz").join("Live"))
        .await
        .unwrap();
    assert_eq!(reference.directory_id, "legacy-music");
    assert_eq!(reference.components, ["Jazz", "Live"]);
    assert_eq!(
        services
            .storage
            .shared_directories(&admin)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_host_directory_outside_every_grant_becomes_a_browsable_connectable_grant() {
    let (dir, services, admin, library, _music) = support::library("server").await;
    let outside = dir.path().join("PRIVATE_OUTSIDE").join("Archive");
    std::fs::create_dir_all(outside.join("Album")).unwrap();
    support::write_wav(&outside.join("Album").join("one.wav"), 3);
    let reference = services
        .storage
        .grant_host_directory(&admin, &outside)
        .await
        .unwrap();
    assert!(reference.components.is_empty());
    let again = services
        .storage
        .grant_host_directory(&admin, &outside)
        .await
        .unwrap();
    assert_eq!(again.directory_id, reference.directory_id);
    let shared = services.storage.shared_directories(&admin).await.unwrap();
    assert_eq!(shared.len(), 2);
    let listing = serde_json::to_string(&shared).unwrap();
    assert!(listing.contains("Archive") && !listing.contains("PRIVATE_OUTSIDE"));
    let page = services
        .storage
        .browse(
            &admin,
            BrowseDirectories {
                directory: reference.clone(),
                offset: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(page.directories[0].name, "Album");
    let folder = services
        .storage
        .connect_folder(
            &admin,
            &library,
            ConnectFolder {
                directory: reference,
                name: None,
            },
        )
        .await
        .unwrap();
    assert!(folder.available);
}

#[tokio::test]
async fn unusable_host_directories_are_rejected_without_creating_a_grant() {
    let (dir, services, admin, _library, _music) = support::library("server").await;
    let file = dir.path().join("plain.txt");
    std::fs::write(&file, b"x").unwrap();
    for path in [file, dir.path().join("missing"), "relative".into()] {
        assert!(
            services
                .storage
                .grant_host_directory(&admin, &path)
                .await
                .is_err()
        );
    }
    assert_eq!(
        services
            .storage
            .shared_directories(&admin)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn listeners_cannot_grant_host_directories() {
    let (dir, services, admin, _library, _music) = support::library("server").await;
    let credentials = || Credentials {
        username: "listener".into(),
        password: "listener storage password".into(),
    };
    services
        .identity
        .create_user(&admin, credentials())
        .await
        .unwrap();
    let session = services.identity.login(credentials()).await.unwrap();
    let listener = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(matches!(
        services
            .storage
            .grant_host_directory(&listener, dir.path())
            .await,
        Err(Error::Forbidden)
    ));
}

#[tokio::test]
async fn a_granted_host_directory_and_its_folder_survive_a_restart() {
    use hito_core::{CoreConfig, HostDeviceId};
    let (dir, services, admin, library, music) = support::library("server").await;
    let outside = dir.path().join("Archive");
    std::fs::create_dir_all(&outside).unwrap();
    let reference = services
        .storage
        .grant_host_directory(&admin, &outside)
        .await
        .unwrap();
    services
        .storage
        .connect_folder(
            &admin,
            &library,
            ConnectFolder {
                directory: reference.clone(),
                name: None,
            },
        )
        .await
        .unwrap();
    drop(services);
    let restarted = hito_core::open(CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("server").unwrap(),
    ))
    .await
    .unwrap();
    let shared = restarted.storage.shared_directories(&admin).await.unwrap();
    assert!(
        shared
            .iter()
            .any(|share| share.id == reference.directory_id && share.available)
    );
    let folders = restarted.storage.folders(&admin, &library).await.unwrap();
    assert!(folders.iter().any(|folder| folder.available));
}

async fn restarted(dir: &std::path::Path, music: std::path::PathBuf) -> hito_core::Services {
    hito_core::open(hito_core::CoreConfig::new(
        dir.join("hito.db"),
        music,
        hito_core::HostDeviceId::new("server").unwrap(),
    ))
    .await
    .unwrap()
}

fn host_id(root: &str) -> String {
    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(root.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("host-{}", &hex[..16])
}

#[tokio::test]
async fn a_removed_folders_host_grant_is_not_restored() {
    let (dir, services, admin, library, music) = support::library("server").await;
    let outside = dir.path().join("Archive");
    std::fs::create_dir_all(&outside).unwrap();
    let reference = services
        .storage
        .grant_host_directory(&admin, &outside)
        .await
        .unwrap();
    let folder = services
        .storage
        .connect_folder(
            &admin,
            &library,
            ConnectFolder {
                directory: reference.clone(),
                name: None,
            },
        )
        .await
        .unwrap();
    services
        .storage
        .remove_folder(&admin, &folder.id, folder.revision)
        .await
        .unwrap();
    drop(services);
    let again = restarted(dir.path(), music).await;
    let shared = again.storage.shared_directories(&admin).await.unwrap();
    assert!(
        shared
            .iter()
            .all(|share| share.id != reference.directory_id)
    );
}

#[tokio::test]
async fn startup_survives_more_restorable_grants_than_the_cap() {
    use hito_core::{CoreConfig, HostDeviceId, SharedDirectoryConfig};
    let (dir, services, admin, library, _music) = support::library("server").await;
    let outside = dir.path().join("Archive");
    std::fs::create_dir_all(&outside).unwrap();
    let reference = services
        .storage
        .grant_host_directory(&admin, &outside)
        .await
        .unwrap();
    services
        .storage
        .connect_folder(
            &admin,
            &library,
            ConnectFolder {
                directory: reference,
                name: None,
            },
        )
        .await
        .unwrap();
    drop(services);
    let grants = (0..64)
        .map(|index| {
            let root = dir.path().join(format!("grant{index}"));
            std::fs::create_dir_all(&root).unwrap();
            SharedDirectoryConfig {
                id: format!("g{index}"),
                name: format!("G{index}"),
                root,
            }
        })
        .collect();
    let full = hito_core::open(CoreConfig::with_shared_directories(
        dir.path().join("hito.db"),
        HostDeviceId::new("server").unwrap(),
        grants,
    ))
    .await
    .unwrap();
    assert_eq!(
        full.storage.shared_directories(&admin).await.unwrap().len(),
        64
    );
}

#[tokio::test]
async fn dangerous_roots_are_refused_with_a_conflict() {
    let (dir, services, admin, _library, _music) = support::library("server").await;
    for path in [std::path::Path::new("/"), dir.path()] {
        assert!(matches!(
            services.storage.grant_host_directory(&admin, path).await,
            Err(Error::Conflict(_))
        ));
    }
    assert_eq!(
        services
            .storage
            .shared_directories(&admin)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_tampered_row_cannot_restore_a_dangerous_root() {
    let (dir, services, admin, library, music) = support::library("server").await;
    let outside = dir.path().join("Archive");
    std::fs::create_dir_all(&outside).unwrap();
    let reference = services
        .storage
        .grant_host_directory(&admin, &outside)
        .await
        .unwrap();
    services
        .storage
        .connect_folder(
            &admin,
            &library,
            ConnectFolder {
                directory: reference,
                name: None,
            },
        )
        .await
        .unwrap();
    drop(services);
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        dir.path().join("hito.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE music_folders SET root_path = '/', directory_id = ? WHERE directory_id LIKE 'host-%'")
        .bind(host_id("/"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let again = restarted(dir.path(), music).await;
    let shared = again.storage.shared_directories(&admin).await.unwrap();
    assert_eq!(shared.len(), 1);
}

#[tokio::test]
async fn a_folder_containing_a_connected_folder_is_refused() {
    let (dir, services, admin, library, _music) = support::library("server").await;
    let parent = dir.path().join("Parent");
    let child = parent.join("Archive");
    std::fs::create_dir_all(&child).unwrap();
    let reference = services
        .storage
        .grant_host_directory(&admin, &child)
        .await
        .unwrap();
    services
        .storage
        .connect_folder(
            &admin,
            &library,
            ConnectFolder {
                directory: reference,
                name: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        services.storage.grant_host_directory(&admin, &parent).await,
        Err(Error::Conflict(_))
    ));
    assert!(
        services
            .storage
            .grant_host_directory(&admin, &child)
            .await
            .is_ok()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn non_utf8_host_paths_are_refused() {
    use std::os::unix::ffi::OsStrExt;
    let (dir, services, admin, _library, _music) = support::library("server").await;
    let odd = dir.path().join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
    std::fs::create_dir_all(&odd).unwrap();
    assert!(
        services
            .storage
            .grant_host_directory(&admin, &odd)
            .await
            .is_err()
    );
    assert_eq!(
        services
            .storage
            .shared_directories(&admin)
            .await
            .unwrap()
            .len(),
        1
    );
}

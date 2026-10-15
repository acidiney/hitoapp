mod support;

use hito_core::{CoreConfig, HostDeviceId, SharedDirectoryConfig};

#[tokio::test]
async fn administrator_sees_shared_directories_without_host_paths() {
    let (dir, _original, actor, _library, music) = support::library("server").await;
    let archive = dir.path().join("PRIVATE_ARCHIVE_DIRECTORY");
    std::fs::create_dir(&archive).unwrap();
    let services = hito_core::open(CoreConfig::with_shared_directories(
        dir.path().join("hito.db"),
        HostDeviceId::new("server").unwrap(),
        vec![
            SharedDirectoryConfig {
                id: "music".into(),
                name: "Music".into(),
                root: music,
            },
            SharedDirectoryConfig {
                id: "archive".into(),
                name: "Archive".into(),
                root: archive,
            },
        ],
    ))
    .await
    .unwrap();
    let shared = services.storage.shared_directories(&actor).await.unwrap();
    assert_eq!(shared.len(), 2);
    assert!(shared.iter().all(|directory| directory.available));
    assert_eq!(shared[0].name, "Music");
    assert_eq!(shared[1].name, "Archive");
    let response = serde_json::to_string(&shared).unwrap();
    assert!(!response.contains("PRIVATE_ARCHIVE_DIRECTORY"));
    assert!(!response.contains(dir.path().to_str().unwrap()));
}

#[tokio::test]
async fn listeners_cannot_inspect_shared_directories() {
    use hito_core::{Credentials, Error};
    let (_dir, services, admin, _library, _music) = support::library("server").await;
    services
        .identity
        .create_user(
            &admin,
            Credentials {
                username: "listener".into(),
                password: "listener storage password".into(),
            },
        )
        .await
        .unwrap();
    let session = services
        .identity
        .login(Credentials {
            username: "listener".into(),
            password: "listener storage password".into(),
        })
        .await
        .unwrap();
    let listener = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(matches!(
        services.storage.shared_directories(&listener).await,
        Err(Error::Forbidden)
    ));
}

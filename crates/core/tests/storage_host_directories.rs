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

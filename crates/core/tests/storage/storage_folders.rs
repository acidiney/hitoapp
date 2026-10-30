use hito_core::test_support::support;

use hito_core::{
    ConnectFolder, CoreConfig, DirectoryRef, HostDeviceId, Search, SharedDirectoryConfig,
};

#[tokio::test]
async fn independently_scanning_one_folder_preserves_another_folders_sources() {
    let (dir, _original, actor, library, music) = support::library("server").await;
    let archive = dir.path().join("archive");
    std::fs::create_dir(&archive).unwrap();
    let track = music.join("first.wav");
    support::write_wav(&track, 0);
    support::write_wav(&archive.join("second.wav"), 1);
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
    let mut folders = Vec::new();
    for id in ["music", "archive"] {
        let folder = services
            .storage
            .connect_folder(
                &actor,
                &library,
                ConnectFolder {
                    directory: DirectoryRef {
                        directory_id: id.into(),
                        components: Vec::new(),
                    },
                    name: None,
                },
            )
            .await
            .unwrap();
        assert!(services.storage.run_next_job().await.unwrap());
        folders.push(folder);
    }
    let recordings = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 2);
    assert!(recordings.items.iter().all(|recording| recording.available));
    std::fs::remove_file(track).unwrap();
    let job = services
        .storage
        .enqueue_folder_scan(&actor, &folders[0].id)
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
    assert!(
        !recordings
            .items
            .iter()
            .find(|recording| recording.title == "first")
            .unwrap()
            .available
    );
    assert!(
        recordings
            .items
            .iter()
            .find(|recording| recording.title == "second")
            .unwrap()
            .available
    );
    let saved = services.storage.folders(&actor, &library).await.unwrap();
    assert_eq!(saved.len(), 2);
    assert!(saved.iter().all(|folder| folder.latest_job.is_some()));
}

#[tokio::test]
async fn removing_and_reconnecting_a_folder_preserves_originals_and_collection_references() {
    use hito_core::Action;
    use uuid::Uuid;
    let (_dir, services, actor, library, music) = support::library("server").await;
    let path = music.join("track.wav");
    let expected = support::write_wav(&path, 0);
    support::scan(&services, &actor, &library).await;
    let folder = services
        .storage
        .folders(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    let recording = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let source = services
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap()
        .remove(0);
    let playlist = Uuid::new_v4();
    support::synchronize(
        &services,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Keep these references".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: Uuid::parse_str(&recording.id).unwrap(),
                position: 0,
            },
            Action::LikeSet {
                recording_id: Uuid::parse_str(&recording.id).unwrap(),
                liked: true,
            },
        ],
    )
    .await;
    services
        .storage
        .remove_folder(&actor, &folder.id, folder.revision)
        .await
        .unwrap();
    assert!(
        services
            .storage
            .folders(&actor, &library)
            .await
            .unwrap()
            .is_empty()
    );
    let current = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(current.total, 1);
    assert_eq!(current.items[0].id, recording.id);
    assert!(current.items[0].liked && !current.items[0].available);
    assert!(matches!(
        services.playback.audio_file(&actor, &source.id).await,
        Err(hito_core::Error::NotFound)
    ));
    assert!(
        !services
            .collection
            .playlist(&actor, &playlist.to_string())
            .await
            .unwrap()
            .entries[0]
            .available
    );
    assert_eq!(std::fs::read(&path).unwrap(), expected);
    let restored = services
        .storage
        .connect_folder(
            &actor,
            &library,
            ConnectFolder {
                directory: folder.directory.unwrap(),
                name: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(restored.id, folder.id);
    assert!(services.storage.run_next_job().await.unwrap());
    let current = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(current.items[0].id, recording.id);
    assert!(current.items[0].liked && current.items[0].available);
    let sources = services
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, source.id);
    assert!(sources[0].ready);
    assert_eq!(
        support::audio_bytes(
            services
                .playback
                .audio_file(&actor, &source.id)
                .await
                .unwrap()
        ),
        expected
    );
}

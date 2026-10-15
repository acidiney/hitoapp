mod support;

use hito_core::{
    ConnectFolder, CoreConfig, DirectoryRef, HostDeviceId, Search, SharedDirectoryConfig,
};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};

#[tokio::test]
async fn disconnecting_a_metadata_owner_moves_recording_credits_to_the_surviving_source() {
    let (dir, _original, actor, library, music) = support::library("credit-host").await;
    let archive = dir.path().join("archive");
    std::fs::create_dir(&archive).unwrap();
    for (folder, artist) in [
        (&music, "Primary performer"),
        (&archive, "Alternate performer"),
    ] {
        let file = folder.join("track.wav");
        support::write_wav(&file, 0);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Same performance".into());
        tag.set_artist(artist.into());
        tag.save_to_path(file, Default::default()).unwrap();
    }
    let hito = hito_core::open(CoreConfig::with_shared_directories(
        dir.path().join("hito.db"),
        HostDeviceId::new("credit-host").unwrap(),
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
        folders.push(
            hito.storage
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
                .unwrap(),
        );
        assert!(hito.storage.run_next_job().await.unwrap());
    }
    let artists = hito
        .catalog
        .artists(&actor, Default::default())
        .await
        .unwrap();
    let primary = &artists
        .items
        .iter()
        .find(|artist| artist.name == "Primary performer")
        .unwrap()
        .id;
    let alternate = &artists
        .items
        .iter()
        .find(|artist| artist.name == "Alternate performer")
        .unwrap()
        .id;
    let first = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(first.total, 1);
    assert_eq!(first.items[0].artist, "Primary performer");
    hito.storage
        .remove_folder(&actor, &folders[0].id, folders[0].revision)
        .await
        .unwrap();
    let current = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(current.items[0].artist, "Alternate performer");
    assert_eq!(
        hito.catalog
            .recordings(
                &actor,
                Search {
                    artist: Some(primary.clone()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );
    let credited = hito
        .catalog
        .recordings(
            &actor,
            Search {
                artist: Some(alternate.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(credited.total, 1);
    assert_eq!(credited.items[0].id, first.items[0].id);
    assert!(credited.items[0].available);
}

mod support;

use hito_core::{CoreConfig, HostDeviceId, Search};

#[tokio::test]
async fn identical_pathnames_on_two_hosts_have_independent_folder_and_source_ownership() {
    let (dir, first, actor, library, music) = support::library("first-host").await;
    support::write_wav(&music.join("track.wav"), 0);
    support::scan(&first, &actor, &library).await;
    let second = hito_core::open(CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("second-host").unwrap(),
    ))
    .await
    .unwrap();
    support::scan(&second, &actor, &library).await;
    let recordings = first
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    for services in [&first, &second] {
        let sources = services
            .catalog
            .sources(&actor, &recordings.items[0].id)
            .await
            .unwrap();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources.iter().filter(|source| source.ready).count(), 1);
        let source = sources.iter().find(|source| source.ready).unwrap();
        assert!(
            services
                .catalog
                .audio_file(&actor, &source.id)
                .await
                .is_ok()
        );
    }
    let folder = first
        .storage
        .folders(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    first
        .storage
        .remove_folder(&actor, &folder.id, folder.revision)
        .await
        .unwrap();
    assert!(
        !first
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .items[0]
            .available
    );
    assert!(
        second
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .items[0]
            .available
    );
}

#[cfg(unix)]
#[tokio::test]
async fn native_file_locators_do_not_conflate_non_utf8_names() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let (_dir, services, actor, library, music) = support::library("server").await;
    for (byte, offset) in [(0xfe, 0), (0xff, 1)] {
        let path = music.join(OsString::from_vec(vec![b'a', byte, b'.', b'w', b'a', b'v']));
        support::write_wav(&path, offset);
    }
    support::scan(&services, &actor, &library).await;
    let before = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(before.total, 2);
    for recording in &before.items {
        let sources = services
            .catalog
            .sources(&actor, &recording.id)
            .await
            .unwrap();
        assert_eq!(sources.len(), 1);
        assert!(sources[0].ready);
        assert!(
            !support::audio_bytes(
                services
                    .catalog
                    .audio_file(&actor, &sources[0].id)
                    .await
                    .unwrap()
            )
            .is_empty()
        );
    }
    support::scan(&services, &actor, &library).await;
    let after = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(after.total, 2);
    assert!(
        after
            .items
            .iter()
            .all(|recording| before.items.iter().any(|old| old.id == recording.id))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn distinct_native_folder_names_do_not_merge_releases() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let (_dir, services, actor, library, music) = support::library("server").await;
    for (byte, offset) in [(0xfe, 0), (0xff, 1)] {
        let folder = music.join(OsString::from_vec(vec![b'a', byte]));
        std::fs::create_dir(&folder).unwrap();
        support::write_wav(&folder.join("track.wav"), offset);
    }
    support::scan(&services, &actor, &library).await;
    let recordings = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 2);
    assert!(recordings.items[0].release_id.is_some());
    assert_ne!(
        recordings.items[0].release_id,
        recordings.items[1].release_id
    );
}

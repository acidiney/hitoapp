use hito_core::test_support::support;

use hito_core::{Action, Browse, Credentials, Error, Operation, Search, SyncBatch};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use support::{catalog, synchronize};
use uuid::Uuid;

#[tokio::test]
async fn personal_title_is_used_by_search_albums_playlists_and_history() {
    let (_dir, hito, actor, _library) = catalog().await;
    let track = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording = Uuid::parse_str(&track.id).unwrap();
    let playlist = Uuid::new_v4();
    synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Personal collection".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: recording,
                position: 0,
            },
            Action::ProgressSet {
                recording_id: recording,
                position_ms: 100,
                speed: 1.0,
            },
            Action::RecordingTitleSet {
                recording_id: recording,
                title: "Étoile personnelle".into(),
            },
        ],
    )
    .await;
    let page = hito
        .catalog
        .recordings(
            &actor,
            Search {
                q: Some("etoile Unknown".into()),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, track.id);
    assert_eq!(page.items[0].title, "Étoile personnelle");
    let all = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(all.items.last().unwrap().id, track.id);
    let old = hito
        .catalog
        .recordings(
            &actor,
            Search {
                q: Some(track.title.clone()),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert!(old.items.iter().all(|item| item.id != track.id));
    assert_eq!(
        hito.collection
            .playlist(&actor, &playlist.to_string())
            .await
            .unwrap()
            .entries[0]
            .title,
        "Étoile personnelle"
    );
    assert_eq!(
        hito.collection.activity(&actor).await.unwrap().history[0].title,
        "Étoile personnelle"
    );
    let album = hito
        .catalog
        .release(
            &actor,
            track.release_id.as_ref().unwrap(),
            Browse::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        album
            .tracks
            .items
            .iter()
            .find(|item| item.recording.id == track.id)
            .unwrap()
            .recording
            .title,
        "Étoile personnelle"
    );
}

#[tokio::test]
async fn title_reset_follows_retags_and_replay_does_not_restore_an_older_edit() {
    let (_dir, hito, actor, library, music) = support::library("title-host").await;
    let path = music.join("private-file-name.wav");
    support::write_wav(&path, 0);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Observed first".into());
    tag.set_artist("First credit".into());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    support::scan(&hito, &actor, &library).await;
    let track = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording = Uuid::parse_str(&track.id).unwrap();
    let bob = hito
        .identity
        .create_user(
            &actor,
            Credentials {
                username: "bob".into(),
                password: "metadata bob password".into(),
            },
        )
        .await
        .unwrap();
    hito.identity
        .set_library_access(&actor, &library, &bob.id, true)
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "bob".into(),
            password: "metadata bob password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    let operation = Uuid::new_v4();
    for _ in 0..2 {
        hito.collection
            .apply(
                &actor,
                SyncBatch {
                    device_id: "offline-client".into(),
                    operations: vec![Operation {
                        id: operation,
                        action: Action::RecordingTitleSet {
                            recording_id: recording,
                            title: "Observed first".into(),
                        },
                    }],
                },
            )
            .await
            .unwrap();
    }
    tag.set_title("Observed second".into());
    tag.set_artist("Second credit".into());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    support::scan(&hito, &actor, &library).await;
    assert_eq!(
        hito.catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .items[0]
            .title,
        "Observed first"
    );
    assert_eq!(
        hito.catalog
            .recordings(&bob, Search::default())
            .await
            .unwrap()
            .items[0]
            .title,
        "Observed second"
    );
    let matches = hito
        .catalog
        .recordings(
            &actor,
            Search {
                q: Some("Observed first Second".into()),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(matches.total, 1);
    synchronize(
        &hito,
        &actor,
        vec![Action::RecordingTitleInherit {
            recording_id: recording,
        }],
    )
    .await;
    hito.collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "offline-client".into(),
                operations: vec![Operation {
                    id: operation,
                    action: Action::RecordingTitleSet {
                        recording_id: recording,
                        title: "Observed first".into(),
                    },
                }],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        hito.catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .items[0]
            .title,
        "Observed second"
    );
    assert_eq!(
        hito.collection
            .events(&actor, 0)
            .await
            .unwrap()
            .operations
            .len(),
        2
    );
    assert!(
        hito.collection
            .events(&bob, 0)
            .await
            .unwrap()
            .operations
            .is_empty()
    );
    tag.set_title("Observed third".into());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    support::scan(&hito, &actor, &library).await;
    assert_eq!(
        hito.catalog
            .recordings(
                &actor,
                Search {
                    q: Some("Observed third".into()),
                    ..Search::default()
                }
            )
            .await
            .unwrap()
            .total,
        1
    );
    hito.identity
        .set_library_access(&actor, &library, &bob.user().id, false)
        .await
        .unwrap();
    let result = hito
        .collection
        .apply(
            &bob,
            SyncBatch {
                device_id: "bob-device".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action: Action::RecordingTitleSet {
                        recording_id: recording,
                        title: "Hidden".into(),
                    },
                }],
            },
        )
        .await;
    assert!(matches!(result, Err(Error::NotFound)));
}

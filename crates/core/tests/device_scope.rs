use hito_core::test_support::support;

use hito_core::{Action, CoreConfig, Credentials, Error, HostDeviceId, Search, open};
use support::{library, scan, synchronize, write_wav};
use uuid::Uuid;

#[tokio::test]
async fn host_owns_local_playability_independently_of_sync_origin() {
    let (dir, owner, actor, library, music) = library("desktop-owner").await;
    write_wav(&music.join("track.wav"), 0);
    scan(&owner, &actor, &library).await;
    let recording = owner
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    assert!(recording.available);
    let source = owner
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(source.device_id.as_deref(), Some("desktop-owner"));
    assert!(source.ready);
    assert!(owner.catalog.audio_file(&actor, &source.id).await.is_ok());

    let playlist = Uuid::new_v4();
    synchronize(
        &owner,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Private playlist".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: Uuid::parse_str(&recording.id).unwrap(),
                position: 0,
            },
            Action::ProgressSet {
                recording_id: Uuid::parse_str(&recording.id).unwrap(),
                position_ms: 500,
                speed: 1.0,
            },
        ],
    )
    .await;

    let other = open(CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("catalog-test").unwrap(),
    ))
    .await
    .unwrap();
    let session = other
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let other_actor = other.identity.authenticate(&session.token).await.unwrap();
    assert!(
        !other
            .catalog
            .recordings(&other_actor, Search::default())
            .await
            .unwrap()
            .items[0]
            .available
    );
    assert!(
        !other
            .catalog
            .sources(&other_actor, &recording.id)
            .await
            .unwrap()[0]
            .ready
    );
    assert!(matches!(
        other.catalog.audio_file(&other_actor, &source.id).await,
        Err(Error::NotFound)
    ));
    assert!(
        !other
            .collection
            .playlist(&other_actor, &playlist.to_string())
            .await
            .unwrap()
            .entries[0]
            .available
    );
    assert!(
        !other
            .collection
            .activity(&other_actor)
            .await
            .unwrap()
            .history[0]
            .available
    );
    assert!(
        owner
            .collection
            .playlist(&actor, &playlist.to_string())
            .await
            .unwrap()
            .entries[0]
            .available
    );
    assert!(owner.collection.activity(&actor).await.unwrap().history[0].available);
}

#[test]
fn host_ids_reject_empty_or_untrusted_identifiers() {
    for value in ["", "../music", "space name", "token=secret"] {
        assert!(matches!(HostDeviceId::new(value), Err(Error::Invalid(_))));
    }
    assert!(HostDeviceId::new("desktop-1_a").is_ok());
}

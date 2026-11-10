mod fixture;

use fixture::{Fixture, PASSWORD, tag_wav, write_wav};
use hito_core::{
    Action, Error, Operation, Search, ServerRelayObservation, SetPin, SyncBatch, Unlock,
};
use uuid::Uuid;

#[tokio::test]
async fn snapshot_pages_belong_to_an_account_and_accept_its_other_current_sessions() {
    let f = Fixture::start().await;
    write_wav(&f.music.join("one.wav"));
    f.scan().await;
    let (owner_session, owner) = f.account("owner").await;
    let (_, other_account) = f.account("other").await;
    let (_, owner_second_session) = f.login("owner").await;
    assert!(!owner.same_session(&owner_second_session));
    let snapshot = f
        .services
        .catalog_replication
        .begin(&owner, &f.library)
        .await
        .unwrap();
    assert_eq!(snapshot.account_id, owner.user().id);
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&other_account, snapshot.id, 0)
            .await,
        Err(Error::NotFound)
    ));
    f.services
        .identity
        .logout(&owner_session.token)
        .await
        .unwrap();
    let page = f
        .services
        .catalog_replication
        .page(&owner_second_session, snapshot.id, 0)
        .await
        .unwrap();
    assert_eq!(page.snapshot_id, snapshot.id);
    assert_eq!(page.recordings.len(), 1);
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn a_revoked_session_cannot_begin_or_read_an_account_snapshot() {
    let f = Fixture::start().await;
    write_wav(&f.music.join("one.wav"));
    f.scan().await;
    let (revoked_session, revoked_actor) = f.account("reader").await;
    let (_, current_actor) = f.login("reader").await;
    f.services
        .identity
        .logout(&revoked_session.token)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .begin(&revoked_actor, &f.library)
            .await,
        Err(Error::Unauthorized)
    ));
    let snapshot = f
        .services
        .catalog_replication
        .begin(&current_actor, &f.library)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&revoked_actor, snapshot.id, 0)
            .await,
        Err(Error::Unauthorized)
    ));
    assert_eq!(
        f.services
            .catalog_replication
            .page(&current_actor, snapshot.id, 0)
            .await
            .unwrap()
            .recordings
            .len(),
        1
    );
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn regranting_library_membership_does_not_restore_an_old_snapshot_grant() {
    let f = Fixture::start().await;
    write_wav(&f.music.join("one.wav"));
    f.scan().await;
    let (_, reader) = f.account("reader").await;
    let snapshot = f
        .services
        .catalog_replication
        .begin(&reader, &f.library)
        .await
        .unwrap();
    f.services
        .identity
        .set_library_access(&f.admin, &f.library, &reader.user().id, false)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&reader, snapshot.id, 0)
            .await,
        Err(Error::NotFound)
    ));
    f.services
        .identity
        .set_library_access(&f.admin, &f.library, &reader.user().id, true)
        .await
        .unwrap();
    f.services.identity.check_session(&reader).await.unwrap();
    assert_eq!(
        f.services
            .catalog
            .recordings(&reader, Search::default())
            .await
            .unwrap()
            .total,
        1
    );
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&reader, snapshot.id, 0)
            .await,
        Err(Error::NotFound)
    ));
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn manual_pin_lock_blocks_retained_actors_until_the_session_is_unlocked() {
    let f = Fixture::start().await;
    write_wav(&f.music.join("one.wav"));
    f.scan().await;
    f.services
        .identity
        .set_pin(
            &f.admin,
            SetPin {
                password: PASSWORD.into(),
                pin: Some("1234".into()),
                idle_minutes: 0,
            },
        )
        .await
        .unwrap();
    assert!(!f.admin.is_locked());
    f.services.identity.lock(&f.admin).await.unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .begin(&f.admin, &f.library)
            .await,
        Err(Error::Locked)
    ));
    f.services
        .identity
        .unlock(&f.admin, Unlock { pin: "1234".into() })
        .await
        .unwrap();
    let snapshot = f
        .services
        .catalog_replication
        .begin(&f.admin, &f.library)
        .await
        .unwrap();
    f.services.identity.lock(&f.admin).await.unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&f.admin, snapshot.id, 0)
            .await,
        Err(Error::Locked)
    ));
    f.services
        .identity
        .unlock(&f.admin, Unlock { pin: "1234".into() })
        .await
        .unwrap();
    assert_eq!(
        f.services
            .catalog_replication
            .page(&f.admin, snapshot.id, 0)
            .await
            .unwrap()
            .recordings
            .len(),
        1
    );
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn ready_pages_preserve_source_release_facts_after_rescan_and_close_invalidates_handles() {
    let f = Fixture::start().await;
    let first = f.music.join("first.wav");
    let second = f.music.join("second.wav");
    for path in [&first, &second] {
        write_wav(path);
    }
    tag_wav(&first, "Opening take", "Performer", "Studio release", 1, 2);
    tag_wav(&second, "Encore take", "Performer", "Live release", 2, 3);
    f.scan().await;
    let mut catalog = f
        .services
        .catalog
        .recordings(&f.admin, Search::default())
        .await
        .unwrap();
    assert_eq!(catalog.total, 1);
    let recording = catalog.items.remove(0);
    f.services
        .collection
        .apply(
            &f.admin,
            SyncBatch {
                device_id: "snapshot-authority-client".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action: Action::RecordingTitleSet {
                        recording_id: Uuid::parse_str(&recording.id).unwrap(),
                        title: "Personal display title".into(),
                    },
                }],
            },
        )
        .await
        .unwrap();
    let snapshot = f
        .services
        .catalog_replication
        .begin(&f.admin, &f.library)
        .await
        .unwrap();
    let page = f
        .services
        .catalog_replication
        .page(&f.admin, snapshot.id, 0)
        .await
        .unwrap();
    assert_eq!(page.recordings.len(), 1);
    let fact = &page.recordings[0];
    assert_eq!(fact.id, recording.id);
    assert_eq!(fact.library_id, f.library);
    assert_eq!(fact.title, recording.inherited_title);
    assert_eq!(fact.artist, recording.inherited_artist);
    assert_ne!(fact.title, "Personal display title");
    assert_eq!(fact.duration_ms, 1_000);
    assert_eq!(fact.sources.len(), 2);
    for (title, album, disc, track) in [
        ("Opening take", "Studio release", 1, 2),
        ("Encore take", "Live release", 2, 3),
    ] {
        let source = fact.sources.iter().find(|s| s.title == title).unwrap();
        assert_eq!(source.artist, "Performer");
        assert_eq!(source.duration_ms, 1_000);
        assert!(source.observation_id.is_some());
        assert_eq!(source.server_relay, ServerRelayObservation::Ready);
        let release = source.release.as_ref().unwrap();
        assert_eq!(release.title, album);
        assert_eq!(release.artist.as_deref(), Some("Release performer"));
        assert_eq!(release.entry_id, source.id);
        assert_eq!(release.disc_number, Some(disc));
        assert_eq!(release.track_number, Some(track));
    }
    assert_ne!(
        fact.sources[0].release.as_ref().unwrap().id,
        fact.sources[1].release.as_ref().unwrap().id
    );
    let retained = serde_json::to_value(&page).unwrap();

    tag_wav(
        &first,
        "Retagged take",
        "Updated performer",
        "Updated release",
        3,
        4,
    );
    std::fs::remove_file(second).unwrap();
    f.scan().await;
    let current = f
        .services
        .catalog
        .recordings(&f.admin, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    assert_eq!(current.id, recording.id);
    assert_eq!(current.inherited_title, "Retagged take");
    assert_eq!(current.inherited_artist, "Updated performer");
    assert_eq!(current.title, "Personal display title");
    assert_eq!(current.album.as_deref(), Some("Updated release"));
    let current_sources = f
        .services
        .catalog
        .sources(&f.admin, &recording.id)
        .await
        .unwrap();
    assert_eq!(current_sources.iter().filter(|s| s.ready).count(), 1);
    assert!(current_sources.iter().any(|s| !s.ready));
    let page_after_rescan = f
        .services
        .catalog_replication
        .page(&f.admin, snapshot.id, 0)
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(page_after_rescan).unwrap(), retained);
    f.services.catalog_replication.close().await.unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&f.admin, snapshot.id, 0)
            .await,
        Err(Error::Internal(_))
    ));
}

use super::fixture::{Fixture, PASSWORD, tag_wav, write_wav};
use hito_core::{Error, SetPin, Unlock};
use uuid::Uuid;

#[tokio::test]
async fn release_allows_a_fresh_snapshot_with_current_facts() {
    let f = Fixture::start().await;
    let wav = f.music.join("one.wav");
    write_wav(&wav);
    tag_wav(&wav, "Original", "Artist", "Release", 1, 1);
    f.scan().await;
    let snapshot = f
        .services
        .catalog_replication
        .begin(&f.admin, &f.library)
        .await
        .unwrap();
    tag_wav(&wav, "Changed", "Artist", "Release", 1, 1);
    f.scan().await;
    f.services
        .catalog_replication
        .release(&f.admin, snapshot.id)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&f.admin, snapshot.id, 0)
            .await,
        Err(Error::NotFound)
    ));
    let fresh = f
        .services
        .catalog_replication
        .begin(&f.admin, &f.library)
        .await
        .unwrap();
    assert_ne!(fresh.id, snapshot.id);
    let page = f
        .services
        .catalog_replication
        .page(&f.admin, fresh.id, 0)
        .await
        .unwrap();
    assert_eq!(page.recordings[0].title, "Changed");
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn releases_are_idempotent_and_only_the_owners_current_sessions_reclaim_a_snapshot() {
    let f = Fixture::populated().await;
    let (session, owner) = f.account("owner").await;
    let (_, foreign) = f.account("foreign").await;
    let (_, alternate) = f.login("owner").await;
    let snapshot = f
        .services
        .catalog_replication
        .begin(&owner, &f.library)
        .await
        .unwrap();
    f.services
        .catalog_replication
        .release(&foreign, snapshot.id)
        .await
        .unwrap();
    f.services
        .catalog_replication
        .release(&foreign, Uuid::new_v4())
        .await
        .unwrap();
    assert_eq!(
        f.services
            .catalog_replication
            .begin(&alternate, &f.library)
            .await
            .unwrap(),
        snapshot
    );
    let foreign_snapshot = f
        .services
        .catalog_replication
        .begin(&foreign, &f.library)
        .await
        .unwrap();
    assert_ne!(foreign_snapshot.id, snapshot.id);
    assert_eq!(foreign_snapshot.account_id, foreign.user().id);
    f.services.identity.logout(&session.token).await.unwrap();
    for id in [snapshot.id, Uuid::new_v4()] {
        assert!(matches!(
            f.services.catalog_replication.release(&owner, id).await,
            Err(Error::Unauthorized)
        ));
    }
    assert!(matches!(
        f.services
            .catalog_replication
            .begin(&owner, &f.library)
            .await,
        Err(Error::Unauthorized)
    ));
    assert_eq!(
        f.services
            .catalog_replication
            .begin(&alternate, &f.library)
            .await
            .unwrap(),
        snapshot
    );
    for id in [snapshot.id, snapshot.id, Uuid::new_v4()] {
        f.services
            .catalog_replication
            .release(&alternate, id)
            .await
            .unwrap();
    }
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&alternate, snapshot.id, 0)
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        f.services
            .catalog_replication
            .page(&foreign, foreign_snapshot.id, 0)
            .await
            .unwrap()
            .recordings
            .len(),
        1
    );
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn pin_lock_prevents_resume_and_release_until_the_current_session_is_unlocked() {
    let f = Fixture::populated().await;
    let snapshot = f
        .services
        .catalog_replication
        .begin(&f.admin, &f.library)
        .await
        .unwrap();
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
    f.services.identity.lock(&f.admin).await.unwrap();
    assert!(!f.admin.is_locked());
    assert!(matches!(
        f.services
            .catalog_replication
            .begin(&f.admin, &f.library)
            .await,
        Err(Error::Locked)
    ));
    for id in [snapshot.id, Uuid::new_v4()] {
        assert!(matches!(
            f.services.catalog_replication.release(&f.admin, id).await,
            Err(Error::Locked)
        ));
    }
    f.services
        .identity
        .unlock(&f.admin, Unlock { pin: "1234".into() })
        .await
        .unwrap();
    assert_eq!(
        f.services
            .catalog_replication
            .begin(&f.admin, &f.library)
            .await
            .unwrap(),
        snapshot
    );
    f.services
        .catalog_replication
        .release(&f.admin, snapshot.id)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&f.admin, snapshot.id, 0)
            .await,
        Err(Error::NotFound)
    ));
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn own_release_accepts_a_snapshot_from_an_older_library_grant() {
    let f = Fixture::populated().await;
    let (_, reader) = f.account("reader").await;
    let snapshot = f
        .services
        .catalog_replication
        .begin(&reader, &f.library)
        .await
        .unwrap();
    for allowed in [false, true] {
        f.services
            .identity
            .set_library_access(&f.admin, &f.library, &reader.user().id, allowed)
            .await
            .unwrap();
    }
    f.services
        .catalog_replication
        .release(&reader, snapshot.id)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .catalog_replication
            .page(&reader, snapshot.id, 0)
            .await,
        Err(Error::NotFound)
    ));
    assert_ne!(
        f.services
            .catalog_replication
            .begin(&reader, &f.library)
            .await
            .unwrap()
            .id,
        snapshot.id
    );
    f.services.catalog_replication.close().await.unwrap();
}

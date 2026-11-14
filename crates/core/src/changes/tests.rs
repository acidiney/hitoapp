use crate::{
    Actor, ChangeNotice, CoreConfig, Credentials, Error, HostDeviceId, ResyncReason, Services,
};
use std::{future::Future, pin::pin, task::Poll};

async fn signed_in(directory: &tempfile::TempDir) -> (Services, Actor, String) {
    let services = crate::open(CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        HostDeviceId::new("change-registration-host").unwrap(),
    ))
    .await
    .unwrap();
    let session = services
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "owned change registration password".into(),
        })
        .await
        .unwrap();
    let actor = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    (services, actor, session.token)
}

fn registrations(services: &Services, actor: &Actor) -> Option<usize> {
    services.changes.hub.registrations(actor.id())
}

async fn guest(services: &Services, owner: &Actor) -> Actor {
    let credentials = || Credentials {
        username: "guest".into(),
        password: "owned change registration guest".into(),
    };
    services
        .identity
        .create_user(owner, credentials())
        .await
        .unwrap();
    let session = services.identity.login(credentials()).await.unwrap();
    services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_cancelled_subscription_retires_its_registration_and_leaves_a_concurrent_stream_alone() {
    let directory = tempfile::tempdir().unwrap();
    let (services, owner, _) = signed_in(&directory).await;
    let guest = guest(&services, &owner).await;
    let mut concurrent = services.changes.subscribe(&guest).await.unwrap();

    let mut held = Vec::new();
    for _ in 0..services.store.pool.options().get_max_connections() {
        held.push(services.store.pool.acquire().await.unwrap());
    }
    {
        let mut subscribing = pin!(services.changes.subscribe(&owner));
        let polled =
            std::future::poll_fn(|context| Poll::Ready(subscribing.as_mut().poll(context))).await;
        assert!(polled.is_pending(), "the session check must be held");
        assert_eq!(registrations(&services, &owner), Some(1));
    }
    assert_eq!(
        registrations(&services, &owner),
        None,
        "a cancelled subscription must not leave its account entry behind"
    );
    assert_eq!(registrations(&services, &guest), Some(1));
    drop(held);

    let mut admitted = Vec::new();
    for _ in 0..8 {
        admitted.push(services.changes.subscribe(&owner).await.unwrap());
    }
    assert!(matches!(
        services.changes.subscribe(&owner).await,
        Err(Error::RateLimited(_))
    ));
    drop(admitted);
    assert_eq!(registrations(&services, &owner), None);

    assert_eq!(
        concurrent.next().await,
        Some(ChangeNotice::Resync {
            reason: ResyncReason::Connected
        })
    );
    services
        .changes
        .hub
        .publish(guest.id(), [ChangeNotice::Playlists]);
    assert_eq!(concurrent.next().await, Some(ChangeNotice::Playlists));
    drop(concurrent);
    assert_eq!(registrations(&services, &guest), None);
}

#[tokio::test]
async fn a_rejected_subscription_retires_its_registration() {
    let directory = tempfile::tempdir().unwrap();
    let (services, actor, token) = signed_in(&directory).await;
    services.identity.logout(&token).await.unwrap();
    assert!(matches!(
        services.changes.subscribe(&actor).await,
        Err(Error::Unauthorized)
    ));
    assert_eq!(registrations(&services, &actor), None);
}

#[tokio::test]
async fn an_admitted_subscription_holds_its_registration_until_the_stream_drops() {
    let directory = tempfile::tempdir().unwrap();
    let (services, actor, _) = signed_in(&directory).await;
    let stream = services.changes.subscribe(&actor).await.unwrap();
    assert_eq!(registrations(&services, &actor), Some(1));
    drop(stream);
    assert_eq!(registrations(&services, &actor), None);
}

async fn library_account(services: &Services, admin: &Actor, username: &str) -> Actor {
    let credentials = || Credentials {
        username: username.into(),
        password: "shared library change reader".into(),
    };
    services
        .identity
        .create_user(admin, credentials())
        .await
        .unwrap();
    let session = services.identity.login(credentials()).await.unwrap();
    services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap()
}

#[tokio::test]
async fn the_recording_readers_helper_lists_current_library_members_only() {
    use crate::test_support::support;
    let (_directory, services, admin, library, music) =
        support::library("change-readers-host").await;
    support::write_wav(&music.join("shared.wav"), 0);
    support::scan(&services, &admin, &library).await;
    let recording: String = sqlx::query_scalar("SELECT id FROM recordings WHERE library_id = ?")
        .bind(&library)
        .fetch_one(&services.store.pool)
        .await
        .unwrap();
    let member = library_account(&services, &admin, "member").await;
    let removed = library_account(&services, &admin, "removed").await;
    let outsider = library_account(&services, &admin, "outsider").await;
    for (account, allowed) in [(&member, true), (&removed, true), (&removed, false)] {
        services
            .identity
            .set_library_access(&admin, &library, account.id(), allowed)
            .await
            .unwrap();
    }
    let mut streams = Vec::new();
    for actor in [&admin, &member, &removed, &outsider] {
        let mut stream = services.changes.subscribe(actor).await.unwrap();
        assert_eq!(
            stream.next().await,
            Some(ChangeNotice::Resync {
                reason: ResyncReason::Connected
            })
        );
        streams.push(stream);
    }

    let readers = crate::identity::recording_readers(
        &mut services.store.pool.acquire().await.unwrap(),
        &recording,
    )
    .await
    .unwrap();
    let mut expected = vec![admin.id().to_owned(), member.id().to_owned()];
    expected.sort();
    assert_eq!(readers, expected);

    let changed = ChangeNotice::Sources {
        recording_id: recording.clone(),
    };
    services
        .changes
        .hub
        .publish_to(&readers, std::slice::from_ref(&changed));
    let mut streams = streams.into_iter();
    for expected in [Some(&changed), Some(&changed), None, None] {
        let mut stream = streams.next().unwrap();
        let received =
            tokio::time::timeout(std::time::Duration::from_millis(200), stream.next()).await;
        match expected {
            Some(notice) => assert_eq!(received.unwrap().as_ref(), Some(notice)),
            None => assert!(received.is_err(), "a non-member must not be notified"),
        }
    }
}

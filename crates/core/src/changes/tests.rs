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
    let actor = services.identity.authenticate(&session.token).await.unwrap();
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
    services.identity.authenticate(&session.token).await.unwrap()
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

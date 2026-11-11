use super::fixture::Case;
use futures_util::future::join_all;
use hito_core::{Actor, Credentials, Error};

#[tokio::test]
async fn five_concurrent_listeners_fit_and_eight_account_leases_cannot_be_oversubscribed() {
    let f = Case::new().await;
    let first =
        join_all((0..5).map(|_| f.services.playback.begin_lease(&f.actor, f.request()))).await;
    let mut leases = first.into_iter().map(Result::unwrap).collect::<Vec<_>>();
    for _ in 0..3 {
        leases.push(
            f.services
                .playback
                .begin_lease(&f.actor, f.request())
                .await
                .unwrap(),
        );
    }
    assert!(matches!(
        f.services.playback.begin_lease(&f.actor, f.request()).await,
        Err(Error::Conflict(_))
    ));
    for lease in leases {
        f.services
            .playback
            .release_lease(&f.actor, lease.id)
            .await
            .unwrap();
    }
    assert!(
        f.services
            .playback
            .begin_lease(&f.actor, f.request())
            .await
            .is_ok()
    );
    f.close().await;
}

#[tokio::test]
async fn concurrent_reservations_cannot_exceed_thirty_two_global_leases() {
    let f = Case::new().await;
    let mut actors = vec![f.actor.clone()];
    for n in 0..4 {
        let credentials = || Credentials {
            username: format!("lease-listener-{n}"),
            password: "owned listener quota password".into(),
        };
        let user = f
            .services
            .identity
            .create_user(&f.actor, credentials())
            .await
            .unwrap();
        f.services
            .identity
            .set_library_access(&f.actor, &f.library, &user.id, true)
            .await
            .unwrap();
        let login = f.services.identity.login(credentials()).await.unwrap();
        actors.push(
            f.services
                .identity
                .authenticate(&login.token)
                .await
                .unwrap(),
        );
    }
    let mut leases = Vec::new();
    for actor in &actors[..3] {
        for _ in 0..8 {
            leases.push((
                actor.clone(),
                f.services
                    .playback
                    .begin_lease(actor, f.request())
                    .await
                    .unwrap(),
            ));
        }
    }
    for result in
        join_all((0..5).map(|_| f.services.playback.begin_lease(&actors[3], f.request()))).await
    {
        leases.push((actors[3].clone(), result.unwrap()));
    }
    let results =
        join_all((0..5).map(|_| f.services.playback.begin_lease(&actors[4], f.request()))).await;
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 3);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(Error::Conflict(_))))
            .count(),
        2
    );
    for lease in results.into_iter().flatten() {
        leases.push((actors[4].clone(), lease));
    }
    assert_eq!(leases.len(), 32);
    assert!(matches!(
        f.services
            .playback
            .begin_lease(&actors[4], f.request())
            .await,
        Err(Error::Conflict(_))
    ));
    let (actor, lease): (Actor, _) = leases.remove(0);
    f.services
        .playback
        .release_lease(&actor, lease.id)
        .await
        .unwrap();
    assert!(
        f.services
            .playback
            .begin_lease(&actor, f.request())
            .await
            .is_ok()
    );
    f.close().await;
}

#[tokio::test]
async fn playback_leases_preserve_catalog_sources_and_original_bytes() {
    let f = Case::new().await;
    let before = serde_json::to_value(
        f.services
            .catalog
            .recordings(&f.actor, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let before_sources = serde_json::to_value(
        f.services
            .catalog
            .sources(&f.actor, &f.recording)
            .await
            .unwrap(),
    )
    .unwrap();
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    f.services
        .playback
        .release_lease(&f.actor, lease.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(
            f.services
                .catalog
                .recordings(&f.actor, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        before
    );
    assert_eq!(
        serde_json::to_value(
            f.services
                .catalog
                .sources(&f.actor, &f.recording)
                .await
                .unwrap()
        )
        .unwrap(),
        before_sources
    );
    assert_eq!(std::fs::read(f.path()).unwrap(), f.original);
    f.close().await;
}

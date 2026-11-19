use super::fixture::{Case, credentials};
use hito_core::{Actor, BeginPlaybackLease, Error};
use uuid::Uuid;

fn tagged(f: &Case, client: Option<&str>, request_id: Option<Uuid>) -> BeginPlaybackLease {
    BeginPlaybackLease {
        client: client.map(str::to_owned),
        request_id,
        ..f.request()
    }
}

async fn second_session(f: &Case) -> Actor {
    let login = f.services.identity.login(credentials()).await.unwrap();
    f.services
        .identity
        .authenticate(&login.token)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_retried_creation_returns_the_same_lease_instead_of_spending_the_quota() {
    let f = Case::new().await;
    let request = Uuid::new_v4();
    let first = f
        .services
        .playback
        .begin_lease(&f.actor, tagged(&f, Some("phone"), Some(request)))
        .await
        .unwrap();
    for _ in 0..12 {
        let again = f
            .services
            .playback
            .begin_lease(&f.actor, tagged(&f, Some("phone"), Some(request)))
            .await
            .unwrap();
        assert_eq!(again, first);
    }
    for _ in 0..7 {
        f.services
            .playback
            .begin_lease(&f.actor, tagged(&f, Some("phone"), Some(Uuid::new_v4())))
            .await
            .unwrap();
    }
    assert!(matches!(
        f.services.playback.begin_lease(&f.actor, f.request()).await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        f.services
            .playback
            .begin_lease(&f.actor, tagged(&f, Some("tablet"), Some(request)))
            .await,
        Err(Error::Invalid(_))
    ));
    f.close().await;
}

#[tokio::test]
async fn a_request_identity_is_never_shared_across_sessions() {
    let f = Case::new().await;
    let other = second_session(&f).await;
    let request = Uuid::new_v4();
    let mine = f
        .services
        .playback
        .begin_lease(&f.actor, tagged(&f, Some("phone"), Some(request)))
        .await
        .unwrap();
    let theirs = f
        .services
        .playback
        .begin_lease(&other, tagged(&f, Some("phone"), Some(request)))
        .await
        .unwrap();
    assert_ne!(mine.id, theirs.id);
    f.close().await;
}

#[tokio::test]
async fn a_restarted_client_releases_what_it_left_behind_and_nothing_else() {
    let f = Case::new().await;
    let other = second_session(&f).await;
    let mut left = Vec::new();
    for _ in 0..5 {
        left.push(
            f.services
                .playback
                .begin_lease(&f.actor, tagged(&f, Some("phone"), None))
                .await
                .unwrap(),
        );
    }
    let tablet = f
        .services
        .playback
        .begin_lease(&f.actor, tagged(&f, Some("tablet"), None))
        .await
        .unwrap();
    let untagged = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let elsewhere = f
        .services
        .playback
        .begin_lease(&other, tagged(&f, Some("phone"), None))
        .await
        .unwrap();
    assert!(matches!(
        f.services.playback.begin_lease(&f.actor, f.request()).await,
        Err(Error::Conflict(_))
    ));

    f.services
        .playback
        .release_client_leases(&other, "tablet")
        .await
        .unwrap();
    f.services
        .playback
        .lease_status(&f.actor, tablet.id)
        .await
        .unwrap();

    f.services
        .playback
        .release_client_leases(&f.actor, "phone")
        .await
        .unwrap();
    for lease in &left {
        assert!(matches!(
            f.services.playback.lease_status(&f.actor, lease.id).await,
            Err(Error::NotFound)
        ));
    }
    for lease in [&tablet, &untagged] {
        f.services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .unwrap();
    }
    f.services
        .playback
        .lease_status(&other, elsewhere.id)
        .await
        .unwrap();
    f.services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    f.services
        .playback
        .release_client_leases(&f.actor, "phone")
        .await
        .unwrap();
    f.close().await;
}

#[tokio::test]
async fn a_client_tag_must_be_short_and_plain() {
    let f = Case::new().await;
    for client in ["", &"x".repeat(65), "phone\n"] {
        assert!(matches!(
            f.services
                .playback
                .begin_lease(&f.actor, tagged(&f, Some(client), None))
                .await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            f.services
                .playback
                .release_client_leases(&f.actor, client)
                .await,
            Err(Error::Invalid(_))
        ));
    }
    assert!(matches!(
        f.services
            .playback
            .begin_lease(&f.actor, tagged(&f, None, Some(Uuid::nil())))
            .await,
        Err(Error::Invalid(_))
    ));
    f.close().await;
}

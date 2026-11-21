use super::fixture::Copies;
use hito_core::{AutomaticCopyBudget, DeleteAccount, Error};

#[tokio::test]
async fn one_accounts_copies_never_reach_another_account() {
    let f = Copies::start().await;
    let next = f.recording("next").await;
    f.announce(&f.actor, &[&next]).await;
    f.drain().await;
    let copy = f.copy_source(&next).await.unwrap();
    let member = f.member().await;

    f.announce(&member, &[&next]).await;
    assert_eq!(f.drain().await, 0);
    assert_eq!(f.server.requests("v1/audio").await.len(), 1);
    assert_eq!(
        f.count("SELECT count(*) FROM upcoming_recordings WHERE account_id<>(SELECT id FROM users WHERE username='admin')")
            .await,
        0
    );
    assert_eq!(
        f.hito
            .downloads
            .automatic_copies(&member)
            .await
            .unwrap()
            .used_bytes,
        0
    );
    assert!(matches!(
        f.hito.playback.playback_source(&member, &next, &copy).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        f.hito.catalog.sources(&member, &next).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        f.hito
            .downloads
            .set_automatic_copy_budget(
                &member,
                &f.actor.user().id,
                AutomaticCopyBudget { budget_bytes: 0 }
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        f.hito
            .downloads
            .automatic_copy_budget(&member, &f.actor.user().id)
            .await,
        Err(Error::Forbidden)
    ));
    assert!(f.copy_source(&next).await.is_some());
}

#[tokio::test]
async fn deleting_an_account_removes_its_copies_and_announcements() {
    let f = Copies::start().await;
    let member = f.member().await;
    f.hito
        .downloads
        .set_automatic_copy_budget(
            &f.actor,
            &member.user().id,
            AutomaticCopyBudget { budget_bytes: 1024 },
        )
        .await
        .unwrap();
    assert_eq!(
        f.hito
            .downloads
            .automatic_copy_budget(&f.actor, &member.user().id)
            .await
            .unwrap()
            .budget_bytes,
        1024
    );
    f.announce(&member, &[]).await;
    f.hito
        .identity
        .delete_account(
            &f.actor,
            &member.user().id,
            DeleteAccount {
                username: "member".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.count("SELECT count(*) FROM account_automatic_copies")
            .await,
        0
    );
    assert!(matches!(
        f.hito
            .downloads
            .automatic_copy_budget(&f.actor, &member.user().id)
            .await,
        Err(Error::NotFound)
    ));
}

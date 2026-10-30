use super::fixture::{World, connect};
use hito_core::{DeleteAccount, Error};

fn named(username: &str) -> DeleteAccount {
    DeleteAccount {
        username: username.into(),
    }
}

#[tokio::test]
async fn only_an_administrator_may_delete_and_unknown_accounts_are_not_found() {
    let world = World::empty().await;
    let admin = world.actor(&world.admin).await;
    let member = world.actor(&world.member).await;
    let outcome = world
        .hito
        .identity
        .delete_account(&member, &world.admin.user.id, named("admin"))
        .await;
    assert!(matches!(outcome, Err(Error::Forbidden)));
    let outcome = world
        .hito
        .identity
        .delete_account(&member, "no-such-account", named("x"))
        .await;
    assert!(matches!(outcome, Err(Error::Forbidden)));
    let outcome = world
        .hito
        .identity
        .delete_account(&admin, "no-such-account", named("erasable-person"))
        .await;
    assert!(matches!(outcome, Err(Error::NotFound)));
    assert_eq!(world.hito.identity.users(&admin).await.unwrap().len(), 2);
}

#[tokio::test]
async fn an_administrator_cannot_delete_their_own_account() {
    let world = World::empty().await;
    let admin = world.actor(&world.admin).await;
    let outcome = world
        .hito
        .identity
        .delete_account(&admin, &world.admin.user.id, named("admin"))
        .await;
    assert!(matches!(outcome, Err(Error::Conflict(_))));
    world.hito.identity.check_session(&admin).await.unwrap();
}

#[tokio::test]
async fn a_stale_or_mistaken_account_name_deletes_nothing() {
    let world = World::empty().await;
    let admin = world.actor(&world.admin).await;
    let outcome = world
        .hito
        .identity
        .delete_account(&admin, &world.member.user.id, named("someone else"))
        .await;
    assert!(matches!(outcome, Err(Error::Conflict(_))));
    world
        .hito
        .identity
        .authenticate(&world.member.token)
        .await
        .unwrap();
}

#[tokio::test]
async fn the_last_administrator_is_never_deleted() {
    let world = World::empty().await;
    let acting = world.actor(&world.admin).await;
    let mut connection = connect(world.directory.path()).await;
    sqlx::query("UPDATE users SET admin=0 WHERE id=?")
        .bind(&world.admin.user.id)
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET admin=1 WHERE id=?")
        .bind(&world.member.user.id)
        .execute(&mut connection)
        .await
        .unwrap();
    let outcome = world
        .hito
        .identity
        .delete_account(&acting, &world.member.user.id, named("erasable-person"))
        .await;
    assert!(
        matches!(outcome, Err(Error::Conflict(_))),
        "removing the only remaining administrator must be refused"
    );
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(remaining, 2);
}

#[tokio::test]
async fn deleting_an_account_revokes_its_sessions_and_keeps_the_administrator_signed_in() {
    let world = World::empty().await;
    let admin = world.actor(&world.admin).await;
    let second = world
        .hito
        .identity
        .login(super::fixture::credentials("erasable-person"))
        .await
        .unwrap();
    let outcome = world
        .hito
        .identity
        .delete_account(&admin, &world.member.user.id, named("erasable-person"))
        .await
        .unwrap();
    assert_eq!(outcome.sessions_revoked, 2);
    for token in [&world.member.token, &second.token] {
        assert!(matches!(
            world.hito.identity.authenticate(token).await,
            Err(Error::Unauthorized)
        ));
    }
    assert!(matches!(
        world
            .hito
            .identity
            .login(super::fixture::credentials("erasable-person"))
            .await,
        Err(Error::Unauthorized)
    ));
    world.hito.identity.check_session(&admin).await.unwrap();
    let again = world
        .hito
        .identity
        .delete_account(&admin, &world.member.user.id, named("erasable-person"))
        .await;
    assert!(matches!(again, Err(Error::NotFound)));
}

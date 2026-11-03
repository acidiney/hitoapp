use hito_core::{
    AuthSession, ChangePassword, CoreConfig, Credentials, Error, HostDeviceId, RemovePin, Services,
    SetPin, open,
};
use sqlx::Connection;

const PASSWORD: &str = "the first long password";
const NEXT: &str = "the second long password";

fn credentials(username: &str, password: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: password.into(),
    }
}

fn change(current: &str, new: &str) -> ChangePassword {
    ChangePassword {
        current_password: current.into(),
        new_password: new.into(),
    }
}

struct World {
    directory: tempfile::TempDir,
    hito: Services,
    first: AuthSession,
    second: AuthSession,
    third: AuthSession,
    other: AuthSession,
}

async fn world() -> World {
    let directory = tempfile::tempdir().unwrap();
    let hito = open(CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        HostDeviceId::new("account-sessions").unwrap(),
    ))
    .await
    .unwrap();
    let first = hito
        .identity
        .setup(credentials("owner", PASSWORD))
        .await
        .unwrap();
    let owner = hito.identity.authenticate(&first.token).await.unwrap();
    hito.identity
        .create_user(&owner, credentials("guest", PASSWORD))
        .await
        .unwrap();
    let second = hito
        .identity
        .login(credentials("owner", PASSWORD))
        .await
        .unwrap();
    let third = hito
        .identity
        .login(credentials("owner", PASSWORD))
        .await
        .unwrap();
    let other = hito
        .identity
        .login(credentials("guest", PASSWORD))
        .await
        .unwrap();
    World {
        directory,
        hito,
        first,
        second,
        third,
        other,
    }
}

impl World {
    async fn actor(&self, session: &AuthSession) -> hito_core::Actor {
        self.hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap()
    }

    async fn alive(&self, session: &AuthSession) -> bool {
        self.hito
            .identity
            .authenticate(&session.token)
            .await
            .is_ok()
    }
}

#[tokio::test]
async fn a_wrong_current_password_is_rejected_generically_and_changes_nothing() {
    let world = world().await;
    let actor = world.actor(&world.first).await;
    let outcome = world
        .hito
        .identity
        .change_password(&actor, change("not the password", NEXT))
        .await;
    assert!(matches!(outcome, Err(Error::WrongPassword)));
    assert!(world.alive(&world.second).await);
    world
        .hito
        .identity
        .login(credentials("owner", PASSWORD))
        .await
        .unwrap();
    assert!(
        world
            .hito
            .identity
            .login(credentials("owner", NEXT))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn wrong_current_passwords_are_counted_and_block_further_guesses() {
    let world = world().await;
    let actor = world.actor(&world.first).await;
    for _ in 0..5 {
        let outcome = world
            .hito
            .identity
            .change_password(&actor, change("not the password", NEXT))
            .await;
        assert!(matches!(outcome, Err(Error::WrongPassword)));
    }
    let blocked = world
        .hito
        .identity
        .change_password(&actor, change(PASSWORD, NEXT))
        .await;
    assert!(matches!(blocked, Err(Error::RateLimited(seconds)) if seconds > 0));
    world
        .hito
        .identity
        .login(credentials("owner", PASSWORD))
        .await
        .unwrap();
    let guest = world.actor(&world.other).await;
    world
        .hito
        .identity
        .change_password(&guest, change(PASSWORD, NEXT))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_successful_change_resets_the_count() {
    let world = world().await;
    let actor = world.actor(&world.first).await;
    for _ in 0..4 {
        let _ = world
            .hito
            .identity
            .change_password(&actor, change("not the password", NEXT))
            .await;
    }
    world
        .hito
        .identity
        .change_password(&actor, change(PASSWORD, NEXT))
        .await
        .unwrap();
    for _ in 0..4 {
        let outcome = world
            .hito
            .identity
            .change_password(&actor, change("not the password", PASSWORD))
            .await;
        assert!(matches!(outcome, Err(Error::WrongPassword)));
    }
}

#[tokio::test]
async fn the_new_password_works_the_old_fails_and_other_sessions_end() {
    let world = world().await;
    let actor = world.actor(&world.first).await;
    let result = world
        .hito
        .identity
        .change_password(&actor, change(PASSWORD, NEXT))
        .await
        .unwrap();
    assert_eq!(result.revoked, 2);
    assert!(world.alive(&world.first).await);
    assert!(!world.alive(&world.second).await);
    assert!(!world.alive(&world.third).await);
    assert!(world.alive(&world.other).await);
    assert!(
        world
            .hito
            .identity
            .login(credentials("owner", PASSWORD))
            .await
            .is_err()
    );
    world
        .hito
        .identity
        .login(credentials("owner", NEXT))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_short_new_password_is_refused_and_keeps_every_session() {
    let world = world().await;
    let actor = world.actor(&world.first).await;
    let outcome = world
        .hito
        .identity
        .change_password(&actor, change(PASSWORD, "short"))
        .await;
    assert!(matches!(outcome, Err(Error::Invalid(_))));
    assert!(world.alive(&world.second).await);
    world
        .hito
        .identity
        .login(credentials("owner", PASSWORD))
        .await
        .unwrap();
}

#[tokio::test]
async fn listing_shows_only_the_accounts_own_sessions_without_secrets() {
    let world = world().await;
    let actor = world.actor(&world.second).await;
    let sessions = world.hito.identity.sessions(&actor).await.unwrap();
    assert_eq!(sessions.len(), 3);
    assert_eq!(sessions.iter().filter(|session| session.current).count(), 1);
    let guest = world.actor(&world.other).await;
    let guest_sessions = world.hito.identity.sessions(&guest).await.unwrap();
    assert_eq!(guest_sessions.len(), 1);
    assert!(
        sessions
            .iter()
            .all(|own| guest_sessions.iter().all(|theirs| own.id != theirs.id))
    );
    let rendered = serde_json::to_string(&sessions).unwrap();
    for session in [&world.first, &world.second, &world.third] {
        assert!(!rendered.contains(&session.token));
    }
    for session in &sessions {
        assert!(session.created_at > 0);
        for secret in [&world.first, &world.second, &world.third] {
            assert_ne!(session.id, secret.token);
            assert!(!secret.token.contains(&session.id));
        }
    }
}

#[tokio::test]
async fn revoking_a_session_of_another_account_is_not_found_and_changes_nothing() {
    let world = world().await;
    let guest = world.actor(&world.other).await;
    let guest_id = world.hito.identity.sessions(&guest).await.unwrap()[0]
        .id
        .clone();
    let owner = world.actor(&world.first).await;
    let outcome = world.hito.identity.revoke_session(&owner, &guest_id).await;
    assert!(matches!(outcome, Err(Error::NotFound)));
    let unknown = world.hito.identity.revoke_session(&owner, "unknown").await;
    assert!(matches!(unknown, Err(Error::NotFound)));
    assert!(world.alive(&world.other).await);
}

#[tokio::test]
async fn one_session_can_be_revoked_and_revoking_the_current_one_signs_out() {
    let world = world().await;
    let owner = world.actor(&world.first).await;
    let sessions = world.hito.identity.sessions(&owner).await.unwrap();
    let current = sessions.iter().find(|session| session.current).unwrap();
    let another = sessions.iter().find(|session| !session.current).unwrap();
    let was_current = world
        .hito
        .identity
        .revoke_session(&owner, &another.id)
        .await
        .unwrap();
    assert!(!was_current);
    assert_eq!(world.hito.identity.sessions(&owner).await.unwrap().len(), 2);
    let was_current = world
        .hito
        .identity
        .revoke_session(&owner, &current.id)
        .await
        .unwrap();
    assert!(was_current);
    assert!(!world.alive(&world.first).await);
}

#[tokio::test]
async fn revoking_other_sessions_keeps_the_current_one_and_other_accounts() {
    let world = world().await;
    let owner = world.actor(&world.second).await;
    let revoked = world
        .hito
        .identity
        .revoke_other_sessions(&owner)
        .await
        .unwrap();
    assert_eq!(revoked.revoked, 2);
    assert!(world.alive(&world.second).await);
    assert!(!world.alive(&world.first).await);
    assert!(!world.alive(&world.third).await);
    assert!(world.alive(&world.other).await);
    assert_eq!(world.hito.identity.sessions(&owner).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_locked_session_cannot_change_the_password_or_end_sessions() {
    let world = world().await;
    let owner = world.actor(&world.first).await;
    world
        .hito
        .identity
        .set_pin(
            &owner,
            hito_core::SetPin {
                password: PASSWORD.into(),
                pin: Some("1234".into()),
                idle_minutes: 0,
            },
        )
        .await
        .unwrap();
    world.hito.identity.lock(&owner).await.unwrap();
    let locked = world.actor(&world.first).await;
    assert!(matches!(
        world
            .hito
            .identity
            .change_password(&locked, change(PASSWORD, NEXT))
            .await,
        Err(Error::Locked)
    ));
    assert!(matches!(
        world.hito.identity.sessions(&locked).await,
        Err(Error::Locked)
    ));
    assert!(matches!(
        world.hito.identity.revoke_other_sessions(&locked).await,
        Err(Error::Locked)
    ));
    assert!(world.alive(&world.second).await);
}

fn pin(password: &str) -> SetPin {
    SetPin {
        password: password.into(),
        pin: Some("1234".into()),
        idle_minutes: 0,
    }
}

#[tokio::test]
async fn pin_routes_and_password_changes_share_one_wrong_password_budget() {
    let world = world().await;
    let actor = world.actor(&world.first).await;
    for _ in 0..3 {
        let outcome = world
            .hito
            .identity
            .set_pin(&actor, pin("not the password"))
            .await;
        assert!(matches!(outcome, Err(Error::WrongPassword)));
    }
    for _ in 0..2 {
        let outcome = world
            .hito
            .identity
            .change_password(&actor, change("not the password", NEXT))
            .await;
        assert!(matches!(outcome, Err(Error::WrongPassword)));
    }
    assert!(matches!(
        world.hito.identity.set_pin(&actor, pin(PASSWORD)).await,
        Err(Error::RateLimited(_))
    ));
    assert!(matches!(
        world
            .hito
            .identity
            .remove_pin(
                &actor,
                RemovePin {
                    password: PASSWORD.into()
                }
            )
            .await,
        Err(Error::RateLimited(_))
    ));
    assert!(matches!(
        world
            .hito
            .identity
            .change_password(&actor, change(PASSWORD, NEXT))
            .await,
        Err(Error::RateLimited(_))
    ));
}

#[tokio::test]
async fn a_correct_pin_password_clears_the_shared_budget() {
    let world = world().await;
    let actor = world.actor(&world.first).await;
    for _ in 0..4 {
        let _ = world
            .hito
            .identity
            .set_pin(&actor, pin("not the password"))
            .await;
    }
    world
        .hito
        .identity
        .set_pin(&actor, pin(PASSWORD))
        .await
        .unwrap();
    for _ in 0..4 {
        let outcome = world
            .hito
            .identity
            .change_password(&actor, change("not the password", NEXT))
            .await;
        assert!(matches!(outcome, Err(Error::WrongPassword)));
    }
}

#[tokio::test]
async fn a_login_in_flight_cannot_outlive_a_password_change() {
    let world = world().await;
    let actor = world.actor(&world.first).await;
    for _ in 0..3 {
        let mut logins = Vec::new();
        for _ in 0..6 {
            let hito = world.hito.clone();
            logins.push(tokio::spawn(async move {
                hito.identity.login(credentials("owner", PASSWORD)).await
            }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let (current, next) = if world
            .hito
            .identity
            .login(credentials("owner", PASSWORD))
            .await
            .is_ok()
        {
            (PASSWORD, NEXT)
        } else {
            (NEXT, PASSWORD)
        };
        world
            .hito
            .identity
            .change_password(&actor, change(current, next))
            .await
            .unwrap();
        for login in logins {
            let _ = login.await.unwrap();
        }
        let survivors = world.hito.identity.sessions(&actor).await.unwrap();
        assert_eq!(survivors.len(), 1, "only the changing session survives");
    }
}

#[tokio::test]
async fn two_racing_changes_leave_one_winner() {
    let world = world().await;
    let actor = world.actor(&world.first).await;
    let (one, two) = tokio::join!(
        world
            .hito
            .identity
            .change_password(&actor, change(PASSWORD, NEXT)),
        world
            .hito
            .identity
            .change_password(&actor, change(PASSWORD, "yet another long password")),
    );
    let wins = [&one, &two].iter().filter(|result| result.is_ok()).count();
    assert_eq!(wins, 1);
    for loser in [one, two].into_iter().filter(|result| result.is_err()) {
        assert!(matches!(
            loser,
            Err(Error::Conflict(_) | Error::WrongPassword)
        ));
    }
}

#[tokio::test]
async fn the_current_session_is_listed_even_beyond_the_list_limit() {
    let world = world().await;
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(world.directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let owner: String = sqlx::query_scalar("SELECT id FROM users WHERE username = 'owner'")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 250)
         INSERT INTO sessions(token_hash, user_id, expires_at)
         SELECT 'filler-' || i, ?, 4102444800 + i FROM n",
    )
    .bind(owner)
    .execute(&mut connection)
    .await
    .unwrap();
    let actor = world.actor(&world.first).await;
    let sessions = world.hito.identity.sessions(&actor).await.unwrap();
    assert_eq!(sessions.len(), 200);
    assert_eq!(sessions.iter().filter(|session| session.current).count(), 1);
}

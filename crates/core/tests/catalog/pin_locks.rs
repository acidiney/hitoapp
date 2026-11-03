use hito_core::test_support::metadata_tag_logs::CapturedLogs;
use hito_core::{
    Actor, AuthSession, CoreConfig, Credentials, Error, HostDeviceId, RemovePin, Services, SetPin,
    Unlock,
};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use tracing::instrument::WithSubscriber;

const PASSWORD: &str = "a long pin test password";

struct Fixture {
    directory: tempfile::TempDir,
    hito: Services,
    admin: AuthSession,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let hito = hito_core::open(CoreConfig::new(
            directory.path().join("hito.db"),
            directory.path().join("music"),
            HostDeviceId::new("pin-lock-test").unwrap(),
        ))
        .await
        .unwrap();
        let admin = hito
            .identity
            .setup(Credentials {
                username: "admin".into(),
                password: PASSWORD.into(),
            })
            .await
            .unwrap();
        Self {
            directory,
            hito,
            admin,
        }
    }

    async fn actor(&self, session: &AuthSession) -> Actor {
        self.hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap()
    }

    async fn sign_in(&self, username: &str) -> AuthSession {
        self.hito
            .identity
            .login(Credentials {
                username: username.into(),
                password: PASSWORD.into(),
            })
            .await
            .unwrap()
    }

    async fn second_account(&self) -> AuthSession {
        let admin = self.actor(&self.admin).await;
        self.hito
            .identity
            .create_user(
                &admin,
                Credentials {
                    username: "bob".into(),
                    password: PASSWORD.into(),
                },
            )
            .await
            .unwrap();
        self.sign_in("bob").await
    }

    async fn set_pin(&self, session: &AuthSession, pin: &str, idle_minutes: u32) {
        let actor = self.actor(session).await;
        self.hito
            .identity
            .set_pin(
                &actor,
                SetPin {
                    password: PASSWORD.into(),
                    pin: Some(pin.into()),
                    idle_minutes,
                },
            )
            .await
            .unwrap();
    }

    async fn locked(&self, session: &AuthSession) -> bool {
        self.actor(session).await.is_locked()
    }

    async fn unlock(&self, session: &AuthSession, pin: &str) -> hito_core::Result<()> {
        let actor = self.actor(session).await;
        self.hito
            .identity
            .unlock(&actor, Unlock { pin: pin.into() })
            .await
            .map(|_| ())
    }

    async fn activity(&self, session: &AuthSession) {
        let actor = self.actor(session).await;
        self.hito.identity.record_activity(&actor).await.unwrap();
    }

    async fn age_sessions(&self, seconds: i64) {
        let mut connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new().filename(self.directory.path().join("hito.db")),
        )
        .await
        .unwrap();
        sqlx::query("UPDATE session_locks SET active_at = active_at - ?")
            .bind(seconds)
            .execute(&mut connection)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn pin_changes_need_the_account_password_and_a_short_numeric_pin() {
    let fixture = Fixture::new().await;
    let identity = &fixture.hito.identity;
    let actor = fixture.actor(&fixture.admin).await;
    let status = identity.lock_status(&actor).await.unwrap();
    assert!(!status.pin_enabled && !status.locked && status.idle_minutes == 0);

    for (password, pin) in [
        ("wrong password entirely", "1234"),
        (PASSWORD, "123"),
        (PASSWORD, "123456789"),
        (PASSWORD, "12a4"),
        (PASSWORD, "１２３４"),
    ] {
        let rejected = identity
            .set_pin(
                &actor,
                SetPin {
                    password: password.into(),
                    pin: Some(pin.into()),
                    idle_minutes: 0,
                },
            )
            .await;
        assert!(
            matches!(rejected, Err(Error::Invalid(_) | Error::WrongPassword)),
            "{pin}"
        );
    }
    assert!(!identity.lock_status(&actor).await.unwrap().pin_enabled);
    assert!(matches!(
        identity
            .set_pin(
                &actor,
                SetPin {
                    password: PASSWORD.into(),
                    pin: Some("1234".into()),
                    idle_minutes: 7,
                },
            )
            .await,
        Err(Error::Invalid(_))
    ));

    fixture.set_pin(&fixture.admin, "1234", 15).await;
    let status = identity.lock_status(&actor).await.unwrap();
    assert!(status.pin_enabled && status.idle_minutes == 15 && !status.locked);

    assert!(matches!(
        identity
            .remove_pin(
                &actor,
                RemovePin {
                    password: "wrong password entirely".into()
                }
            )
            .await,
        Err(Error::WrongPassword)
    ));
    assert!(identity.lock_status(&actor).await.unwrap().pin_enabled);
    identity
        .remove_pin(
            &actor,
            RemovePin {
                password: PASSWORD.into(),
            },
        )
        .await
        .unwrap();
    assert!(!identity.lock_status(&actor).await.unwrap().pin_enabled);
}

#[tokio::test]
async fn the_idle_period_can_change_without_replacing_the_pin() {
    let fixture = Fixture::new().await;
    fixture.set_pin(&fixture.admin, "2468", 0).await;
    let actor = fixture.actor(&fixture.admin).await;
    let status = fixture
        .hito
        .identity
        .set_pin(
            &actor,
            SetPin {
                password: PASSWORD.into(),
                pin: None,
                idle_minutes: 5,
            },
        )
        .await
        .unwrap();
    assert_eq!(status.idle_minutes, 5);
    fixture.hito.identity.lock(&actor).await.unwrap();
    fixture.unlock(&fixture.admin, "2468").await.unwrap();
    let fresh = Fixture::new().await;
    let actor = fresh.actor(&fresh.admin).await;
    assert!(matches!(
        fresh
            .hito
            .identity
            .set_pin(
                &actor,
                SetPin {
                    password: PASSWORD.into(),
                    pin: None,
                    idle_minutes: 5,
                },
            )
            .await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn locking_needs_a_pin_and_unlocking_restores_the_same_session() {
    let fixture = Fixture::new().await;
    let actor = fixture.actor(&fixture.admin).await;
    assert!(matches!(
        fixture.hito.identity.lock(&actor).await,
        Err(Error::Invalid(_))
    ));
    assert!(!fixture.locked(&fixture.admin).await);

    fixture.set_pin(&fixture.admin, "4321", 0).await;
    fixture.hito.identity.lock(&actor).await.unwrap();
    let locked = fixture.actor(&fixture.admin).await;
    assert!(locked.is_locked());
    assert!(matches!(locked.require_unlocked(), Err(Error::Locked)));
    assert_eq!(Error::Locked.code(), "locked");
    assert!(
        fixture
            .hito
            .identity
            .lock_status(&locked)
            .await
            .unwrap()
            .locked
    );

    fixture.unlock(&fixture.admin, "4321").await.unwrap();
    let restored = fixture.actor(&fixture.admin).await;
    assert!(!restored.is_locked() && restored.require_unlocked().is_ok());
    assert!(restored.same_session(&locked));
}

#[tokio::test]
async fn wrong_pins_count_and_the_fifth_ends_the_session() {
    let fixture = Fixture::new().await;
    fixture.set_pin(&fixture.admin, "4321", 0).await;
    let actor = fixture.actor(&fixture.admin).await;
    fixture.hito.identity.lock(&actor).await.unwrap();

    for _ in 0..4 {
        assert!(matches!(
            fixture.unlock(&fixture.admin, "0000").await,
            Err(Error::Invalid(_))
        ));
        assert!(fixture.locked(&fixture.admin).await);
    }
    assert!(matches!(
        fixture.unlock(&fixture.admin, "0000").await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        fixture
            .hito
            .identity
            .authenticate(&fixture.admin.token)
            .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        fixture
            .hito
            .identity
            .unlock(&actor, Unlock { pin: "4321".into() })
            .await,
        Err(Error::Unauthorized)
    ));
    let again = fixture.sign_in("admin").await;
    assert!(
        !fixture.locked(&again).await,
        "a fresh sign-in is not locked"
    );
}

#[tokio::test]
async fn a_correct_pin_resets_the_failure_count() {
    let fixture = Fixture::new().await;
    fixture.set_pin(&fixture.admin, "4321", 0).await;
    let actor = fixture.actor(&fixture.admin).await;
    for _ in 0..2 {
        fixture.hito.identity.lock(&actor).await.unwrap();
        for _ in 0..4 {
            assert!(fixture.unlock(&fixture.admin, "9999").await.is_err());
        }
        fixture.unlock(&fixture.admin, "4321").await.unwrap();
    }
    assert!(
        fixture
            .hito
            .identity
            .authenticate(&fixture.admin.token)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn idle_sessions_lock_after_the_chosen_period_only() {
    let fixture = Fixture::new().await;
    fixture.set_pin(&fixture.admin, "1357", 5).await;
    assert!(!fixture.locked(&fixture.admin).await);
    fixture.age_sessions(4 * 60).await;
    assert!(!fixture.locked(&fixture.admin).await);
    fixture.age_sessions(6 * 60).await;
    assert!(fixture.locked(&fixture.admin).await);
    assert!(fixture.locked(&fixture.admin).await, "it stays locked");
    fixture.unlock(&fixture.admin, "1357").await.unwrap();
    assert!(!fixture.locked(&fixture.admin).await);

    let never = Fixture::new().await;
    never.set_pin(&never.admin, "1357", 0).await;
    never.age_sessions(7 * 24 * 3600).await;
    assert!(!never.locked(&never.admin).await);
}

#[tokio::test]
async fn locks_belong_to_one_session_of_one_account() {
    let fixture = Fixture::new().await;
    let other = fixture.sign_in("admin").await;
    let bob = fixture.second_account().await;
    fixture.set_pin(&fixture.admin, "1111", 0).await;
    fixture.set_pin(&bob, "2222", 0).await;

    let actor = fixture.actor(&fixture.admin).await;
    fixture.hito.identity.lock(&actor).await.unwrap();
    assert!(fixture.locked(&fixture.admin).await);
    assert!(!fixture.locked(&other).await);
    assert!(!fixture.locked(&bob).await);

    assert!(fixture.unlock(&fixture.admin, "2222").await.is_err());
    assert!(fixture.locked(&fixture.admin).await);
    let bob_actor = fixture.actor(&bob).await;
    fixture.hito.identity.lock(&bob_actor).await.unwrap();
    assert!(fixture.unlock(&bob, "1111").await.is_err());
    fixture.unlock(&bob, "2222").await.unwrap();
    assert!(fixture.locked(&fixture.admin).await);

    let other_actor = fixture.actor(&other).await;
    fixture
        .hito
        .identity
        .remove_pin(
            &other_actor,
            RemovePin {
                password: PASSWORD.into(),
            },
        )
        .await
        .unwrap();
    assert!(
        !fixture.locked(&fixture.admin).await,
        "removing the pin unlocks every session"
    );
}

#[tokio::test]
async fn lock_workflows_log_outcomes_without_pins_hashes_or_tokens() {
    let fixture = Fixture::new().await;
    let captured = CapturedLogs::default();
    let dispatch = tracing::Dispatch::new(captured.subscriber());
    async {
        fixture.set_pin(&fixture.admin, "86420", 5).await;
        let actor = fixture.actor(&fixture.admin).await;
        fixture.hito.identity.lock(&actor).await.unwrap();
        let _ = fixture.unlock(&fixture.admin, "13579").await;
        fixture.unlock(&fixture.admin, "86420").await.unwrap();
        fixture.hito.identity.lock(&actor).await.unwrap();
        for _ in 0..5 {
            let _ = fixture.unlock(&fixture.admin, "11112").await;
        }
    }
    .with_subscriber(dispatch)
    .await;
    let text = captured.text();
    for event in [
        "pin_set",
        "session_locked",
        "unlock_rejected",
        "session_unlocked",
        "session_ended_after_wrong_pins",
    ] {
        assert!(text.contains(event), "{event} is logged");
    }
    let token_hash = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(fixture.admin.token.as_bytes())
    );
    for secret in [
        "86420",
        "13579",
        "11112",
        PASSWORD,
        &fixture.admin.token,
        &token_hash,
        "argon2",
    ] {
        assert!(!text.contains(secret), "logs must not contain {secret}");
    }
}

#[tokio::test]
async fn background_requests_do_not_keep_a_session_awake_but_the_activity_signal_does() {
    let fixture = Fixture::new().await;
    fixture.set_pin(&fixture.admin, "1357", 5).await;
    fixture.age_sessions(3 * 60).await;
    for _ in 0..3 {
        assert!(!fixture.locked(&fixture.admin).await);
        let actor = fixture.actor(&fixture.admin).await;
        fixture.hito.identity.check_session(&actor).await.unwrap();
    }
    fixture.age_sessions(3 * 60).await;
    assert!(
        fixture.locked(&fixture.admin).await,
        "background requests never refresh the idle clock"
    );
    fixture.unlock(&fixture.admin, "1357").await.unwrap();

    fixture.age_sessions(4 * 60).await;
    fixture.activity(&fixture.admin).await;
    fixture.age_sessions(4 * 60).await;
    assert!(!fixture.locked(&fixture.admin).await);
    fixture.age_sessions(2 * 60).await;
    assert!(fixture.locked(&fixture.admin).await);
    fixture.activity(&fixture.admin).await;
    assert!(
        fixture.locked(&fixture.admin).await,
        "activity cannot revive a locked session"
    );
}

#[tokio::test]
async fn unlock_pins_that_are_not_four_to_eight_digits_count_without_hashing() {
    let fixture = Fixture::new().await;
    fixture.set_pin(&fixture.admin, "4321", 0).await;
    let actor = fixture.actor(&fixture.admin).await;
    fixture.hito.identity.lock(&actor).await.unwrap();
    for pin in ["", "12", "12a4", "１２３４", &"9".repeat(5000)] {
        assert!(matches!(
            fixture.unlock(&fixture.admin, pin).await,
            Err(Error::Invalid(_)) | Err(Error::Unauthorized)
        ));
    }
    assert!(
        matches!(
            fixture
                .hito
                .identity
                .authenticate(&fixture.admin.token)
                .await,
            Err(Error::Unauthorized)
        ),
        "five malformed attempts used the five allowed"
    );
}

#[tokio::test]
async fn parallel_wrong_guesses_verify_at_most_five_pins_and_end_the_session() {
    let fixture = Fixture::new().await;
    fixture.set_pin(&fixture.admin, "4321", 0).await;
    let actor = fixture.actor(&fixture.admin).await;
    fixture.hito.identity.lock(&actor).await.unwrap();
    let identity = fixture.hito.identity.clone();
    let attempts = (0..12).map(|_| {
        let identity = identity.clone();
        let actor = actor.clone();
        tokio::spawn(async move { identity.unlock(&actor, Unlock { pin: "0000".into() }).await })
    });
    let mut rejected = 0;
    let mut ended = 0;
    for attempt in attempts {
        match attempt.await.unwrap() {
            Err(Error::Invalid(_)) => rejected += 1,
            Err(Error::Unauthorized) => ended += 1,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(rejected, 4, "only four wrong PINs are reported as rejected");
    assert_eq!(
        ended, 8,
        "the fifth wrong PIN and every later attempt end or find the session ended"
    );
    assert!(matches!(
        fixture
            .hito
            .identity
            .authenticate(&fixture.admin.token)
            .await,
        Err(Error::Unauthorized)
    ));
}

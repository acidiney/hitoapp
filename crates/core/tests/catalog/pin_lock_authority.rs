use hito_core::test_support::playback::LocalPlayback;
use hito_core::{Error, SetPin, Unlock};

const PASSWORD: &str = "indexing test password";

async fn locked_fixture() -> LocalPlayback {
    let f = LocalPlayback::start().await;
    f.services
        .identity
        .set_pin(
            &f.actor,
            SetPin {
                password: PASSWORD.into(),
                pin: Some("1234".into()),
                idle_minutes: 5,
            },
        )
        .await
        .unwrap();
    f
}

#[tokio::test]
async fn locking_refuses_retained_actors_and_opened_playback_grants() {
    let f = locked_fixture().await;
    let grant = f
        .services
        .playback
        .playback_source(&f.actor, &f.recording, &f.source)
        .await
        .unwrap();
    grant.check().await.unwrap();
    f.services.identity.check_session(&f.actor).await.unwrap();

    f.services.identity.lock(&f.actor).await.unwrap();
    assert!(matches!(
        f.services.identity.check_session(&f.actor).await,
        Err(Error::Locked)
    ));
    assert!(matches!(grant.check().await, Err(Error::Locked)));
    assert!(matches!(
        f.services
            .playback
            .playback_source(&f.actor, &f.recording, &f.source)
            .await,
        Err(Error::Locked)
    ));

    f.services
        .identity
        .unlock(&f.actor, Unlock { pin: "1234".into() })
        .await
        .unwrap();
    f.services.identity.check_session(&f.actor).await.unwrap();
}

#[tokio::test]
async fn an_expired_idle_period_refuses_retained_actors_before_any_new_request() {
    let f = locked_fixture().await;
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE session_locks SET active_at = active_at - 600")
        .execute(&mut connection)
        .await
        .unwrap();
    assert!(matches!(
        f.services.identity.check_session(&f.actor).await,
        Err(Error::Locked)
    ));
}

use sqlx::Connection;

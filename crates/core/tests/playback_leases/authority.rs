use super::fixture::{Case, credentials};
use hito_core::{
    AudioRequest, BeginPlaybackLease, Credentials, Error, OpenedAudio, SetPin, Unlock,
};

#[tokio::test]
async fn exact_recording_source_and_account_access_are_required() {
    let f = Case::new().await;
    for request in [
        BeginPlaybackLease {
            recording_id: "missing".into(),
            source_id: f.source.clone(),
        },
        BeginPlaybackLease {
            recording_id: f.recording.clone(),
            source_id: "missing".into(),
        },
    ] {
        assert!(matches!(
            f.services.playback.begin_lease(&f.actor, request).await,
            Err(Error::NotFound)
        ));
    }
    let user = f
        .services
        .identity
        .create_user(
            &f.actor,
            Credentials {
                username: "listener".into(),
                password: "owned listener password".into(),
            },
        )
        .await
        .unwrap();
    let login = f
        .services
        .identity
        .login(Credentials {
            username: "listener".into(),
            password: "owned listener password".into(),
        })
        .await
        .unwrap();
    let other = f
        .services
        .identity
        .authenticate(&login.token)
        .await
        .unwrap();
    assert!(matches!(
        f.services.playback.begin_lease(&other, f.request()).await,
        Err(Error::NotFound)
    ));
    f.services
        .identity
        .set_library_access(&f.actor, &f.library, &user.id, true)
        .await
        .unwrap();
    let lease = f
        .services
        .playback
        .begin_lease(&other, f.request())
        .await
        .unwrap();
    assert!(matches!(
        f.services.playback.lease_status(&f.actor, lease.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        f.services.playback.release_lease(&f.actor, lease.id).await,
        Err(Error::NotFound)
    ));
    f.close().await;
}

#[tokio::test]
async fn removing_and_restoring_membership_cannot_revive_a_lease_or_retained_body() {
    let f = Case::new().await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let OpenedAudio::Content(mut audio) = f
        .services
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("body")
    };
    let mut body = audio.body.take().unwrap();
    f.services
        .identity
        .set_library_access(&f.actor, &f.library, &f.actor.user().id, false)
        .await
        .unwrap();
    f.services
        .identity
        .set_library_access(&f.actor, &f.library, &f.actor.user().id, true)
        .await
        .unwrap();
    assert!(body.chunk().await.is_err());
    assert!(
        f.services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .is_err()
    );
    assert!(
        f.services
            .playback
            .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
            .await
            .is_err()
    );
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
async fn current_pin_lock_checks_reject_a_previously_unlocked_actor_and_old_lease_stays_dead() {
    let f = Case::new().await;
    f.services
        .identity
        .set_pin(
            &f.actor,
            SetPin {
                password: credentials().password,
                pin: Some("123456".into()),
                idle_minutes: 0,
            },
        )
        .await
        .unwrap();
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    f.services.identity.lock(&f.actor).await.unwrap();
    assert!(!f.actor.is_locked(), "fixture retains the pre-lock actor");
    assert!(matches!(
        f.services.playback.begin_lease(&f.actor, f.request()).await,
        Err(Error::Locked)
    ));
    assert!(matches!(
        f.services.playback.lease_status(&f.actor, lease.id).await,
        Err(Error::Locked)
    ));
    let locked = f.services.identity.authenticate(&f.token).await.unwrap();
    assert!(matches!(
        f.services.playback.begin_lease(&locked, f.request()).await,
        Err(Error::Locked)
    ));
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    f.services
        .identity
        .unlock(
            &f.actor,
            Unlock {
                pin: "123456".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        f.services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .is_err()
    );
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
async fn logout_cancels_unpolled_bodies_and_a_retained_actor_cannot_reopen() {
    let f = Case::new().await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let OpenedAudio::Content(mut audio) = f
        .services
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("body")
    };
    let mut body = audio.body.take().unwrap();
    f.services.identity.logout(&f.token).await.unwrap();
    assert!(body.chunk().await.is_err());
    assert!(matches!(
        f.services.playback.begin_lease(&f.actor, f.request()).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        f.services.playback.lease_status(&f.actor, lease.id).await,
        Err(Error::Unauthorized)
    ));
    f.close().await;
}

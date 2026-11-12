use super::work::fixture::{Case, credentials};
use crate::{Actor, AudioRequest, Error, OpenedAudio, Result, SetPin, Unlock};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
enum Operation {
    Status,
    Open,
    Release,
}
const OPERATIONS: [Operation; 3] = [Operation::Status, Operation::Open, Operation::Release];

async fn request(f: &Case, actor: &Actor, lease: Uuid, operation: Operation) -> Result<()> {
    match operation {
        Operation::Status => f
            .services
            .playback
            .lease_status(actor, lease)
            .await
            .map(|_| ()),
        Operation::Open => f
            .services
            .playback
            .open_lease_audio(actor, lease, AudioRequest::default())
            .await
            .map(|_| ()),
        Operation::Release => f.services.playback.release_lease(actor, lease).await,
    }
}

async fn set_pin(f: &Case) {
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
}

#[tokio::test]
async fn observed_pin_lock_permanently_retires_the_owning_lease_while_its_read_is_held() {
    let mut evidence = Vec::new();
    for operation in OPERATIONS {
        let f = Case::new().await;
        set_pin(&f).await;
        let pause = Arc::new(super::testing::ReadPause::default());
        f.services.playback.leases.state.lock().unwrap().read_pause = Some(pause.clone());
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
        let reading = tokio::spawn(async move { body.chunk().await });
        pause.entered().await;
        f.services.identity.lock(&f.actor).await.unwrap();
        let previously_unlocked = !f.actor.is_locked();
        let observed_locked = matches!(
            request(&f, &f.actor, lease.id, operation).await,
            Err(Error::Locked)
        );
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
        pause.release();
        let read_denied = reading.await.unwrap().is_err();
        let lease_denied = f
            .services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .is_err();
        let fresh_allowed = f
            .services
            .playback
            .begin_lease(&f.actor, f.request())
            .await
            .is_ok();
        f.close().await;
        evidence.push((
            operation,
            previously_unlocked,
            observed_locked,
            read_denied,
            lease_denied,
            fresh_allowed,
        ));
    }
    assert!(
        evidence.iter().all(
            |(_, old_actor, locked, read_denied, lease_denied, fresh)| *old_actor
                && *locked
                && *read_denied
                && *lease_denied
                && *fresh
        ),
        "observed session failure must remain terminal after unlock: {evidence:?}"
    );
}

#[tokio::test]
async fn another_locked_session_cannot_retire_the_owners_lease_or_change_auth_error_semantics() {
    let mut evidence = Vec::new();
    for operation in OPERATIONS {
        let f = Case::new().await;
        set_pin(&f).await;
        let login = f.services.identity.login(credentials()).await.unwrap();
        let other = f
            .services
            .identity
            .authenticate(&login.token)
            .await
            .unwrap();
        f.services
            .identity
            .unlock(
                &other,
                Unlock {
                    pin: "123456".into(),
                },
            )
            .await
            .unwrap();
        let other = f
            .services
            .identity
            .authenticate(&login.token)
            .await
            .unwrap();
        let distinct_session = !other.same_session(&f.actor)
            && other.user().id == f.actor.user().id
            && !other.is_locked();
        let pause = Arc::new(super::testing::ReadPause::default());
        f.services.playback.leases.state.lock().unwrap().read_pause = Some(pause.clone());
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
        let reading = tokio::spawn(async move { body.chunk().await });
        pause.entered().await;
        f.services.identity.lock(&other).await.unwrap();
        let foreign_locked = matches!(
            request(&f, &other, lease.id, operation).await,
            Err(Error::Locked)
        );
        let missing_locked = matches!(
            request(&f, &other, Uuid::new_v4(), operation).await,
            Err(Error::Locked)
        );
        f.services
            .identity
            .unlock(
                &other,
                Unlock {
                    pin: "123456".into(),
                },
            )
            .await
            .unwrap();
        pause.release();
        let read_unchanged = matches!(reading.await.unwrap(),Ok(Some(bytes)) if bytes.as_ref() == f.original.as_slice());
        let owner_available = f
            .services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .is_ok();
        let foreign_not_found = matches!(
            request(&f, &other, lease.id, operation).await,
            Err(Error::NotFound)
        );
        f.close().await;
        evidence.push((
            operation,
            distinct_session,
            foreign_locked,
            missing_locked,
            read_unchanged,
            owner_available,
            foreign_not_found,
        ));
    }
    assert!(
        evidence.iter().all(
            |(_, distinct, foreign_locked, missing_locked, read, owner, foreign_hidden)| *distinct
                && *foreign_locked
                && *missing_locked
                && *read
                && *owner
                && *foreign_hidden
        ),
        "authorization failure must neither reveal nor retire another exact session's lease: {evidence:?}"
    );
}

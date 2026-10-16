#[path = "fixtures/playback.rs"]
mod fixture;
mod support;
use hito_core::Credentials;

#[tokio::test]
async fn native_owners_can_distinguish_sessions_without_obtaining_their_fingerprints() {
    let f = fixture::LocalPlayback::start().await;
    let same = f.services.identity.authenticate(&f.token).await.unwrap();
    assert!(f.actor.same_session(&same));
    assert!(f.actor.same_session(&f.actor.clone()));
    let newer = f
        .services
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let newer_actor = f
        .services
        .identity
        .authenticate(&newer.token)
        .await
        .unwrap();
    assert!(!f.actor.same_session(&newer_actor));
    f.services.identity.check_session(&f.actor).await.unwrap();
    f.services.identity.logout(&f.token).await.unwrap();
    assert!(matches!(
        f.services.identity.check_session(&f.actor).await,
        Err(hito_core::Error::Unauthorized)
    ));
    f.services
        .identity
        .check_session(&newer_actor)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_retained_native_owner_cannot_save_progress_after_its_session_is_revoked() {
    let f = fixture::LocalPlayback::start().await;
    let recording = uuid::Uuid::parse_str(&f.recording).unwrap();
    support::synchronize(
        &f.services,
        &f.actor,
        vec![hito_core::Action::ProgressSet {
            recording_id: recording,
            position_ms: 10,
            speed: 1.0,
        }],
    )
    .await;
    f.services.identity.logout(&f.token).await.unwrap();
    let result = f
        .services
        .collection
        .apply(
            &f.actor,
            hito_core::SyncBatch {
                device_id: "native-owner".into(),
                operations: vec![hito_core::Operation {
                    id: uuid::Uuid::new_v4(),
                    action: hito_core::Action::ProgressSet {
                        recording_id: recording,
                        position_ms: 999,
                        speed: 1.0,
                    },
                }],
            },
        )
        .await;
    assert!(matches!(result, Err(hito_core::Error::Unauthorized)));
    let session = f
        .services
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let current = f
        .services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert_eq!(
        f.services
            .collection
            .activity(&current)
            .await
            .unwrap()
            .history[0]
            .position_ms,
        10
    );
}

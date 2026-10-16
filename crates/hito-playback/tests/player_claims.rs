#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use uuid::Uuid;

#[tokio::test]
async fn inspection_never_claims_and_only_one_claim_accepts_an_observed_epoch() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let first = player.inspect(&f.actor).await.unwrap();
    let second = player.inspect(&f.actor).await.unwrap();
    assert_eq!(first.epoch, second.epoch);
    assert!(first.state.is_none());
    let (left, right) = tokio::join!(
        player.claim(&f.actor, first.epoch),
        player.claim(&f.actor, first.epoch)
    );
    let attached = match (left, right) {
        (Ok(attached), Err(PlayerError::Conflict)) | (Err(PlayerError::Conflict), Ok(attached)) => {
            attached
        }
        _ => panic!("exactly one simultaneous claim may accept an epoch"),
    };
    assert_ne!(attached.epoch, first.epoch);
    assert_eq!(attached.state.queue_len, 0);
    assert!(matches!(
        player.claim(&f.actor, first.epoch).await,
        Err(PlayerError::Conflict)
    ));
    let seen = player.inspect(&f.actor).await.unwrap();
    assert_eq!(seen.epoch, attached.epoch);
    assert_eq!(seen.state.unwrap().revision, attached.state.revision);
    owner.close().await.unwrap();
    assert!(matches!(
        player.inspect(&f.actor).await,
        Err(PlayerError::Closed)
    ));
}

#[tokio::test]
async fn reattachment_rotates_the_lease_and_distinct_login_sessions_get_private_observations() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let seen = player.inspect(&f.actor).await.unwrap();
    let first = player.claim(&f.actor, seen.epoch).await.unwrap();
    let mut old = player.subscribe(&f.actor, first.lease).await.unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: first.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    old.borrow_and_update();
    let attached = player.claim(&f.actor, first.epoch).await.unwrap();
    assert_eq!(attached.state.queue_len, 1);
    assert_ne!(first.lease, attached.lease);
    assert!(old.changed().await.is_err());
    assert!(matches!(
        player.subscribe(&f.actor, first.lease).await,
        Err(PlayerError::Conflict)
    ));
    let mut current = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let login = f
        .services
        .identity
        .login(hito_core::Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let another = f
        .services
        .identity
        .authenticate(&login.token)
        .await
        .unwrap();
    let private = player.inspect(&another).await.unwrap();
    assert!(private.state.is_none());
    assert!(matches!(
        player.subscribe(&another, attached.lease).await,
        Err(PlayerError::Conflict)
    ));
    let next = player.claim(&another, private.epoch).await.unwrap();
    assert_eq!(next.state.queue_len, 0);
    assert!(next.state.current.is_none());
    assert!(current.changed().await.is_err());
    assert!(player.inspect(&f.actor).await.unwrap().state.is_none());
    assert!(matches!(
        player.subscribe(&f.actor, next.lease).await,
        Err(PlayerError::Conflict)
    ));
    owner.close().await.unwrap();
}

#[tokio::test]
async fn dropping_the_lifecycle_owner_closes_observation_and_admission() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let mut observed = player.subscribe(&f.actor, attached.lease).await.unwrap();
    drop(owner);
    tokio::time::timeout(std::time::Duration::from_secs(2), observed.changed())
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(
        player.inspect(&f.actor).await,
        Err(PlayerError::Closed)
    ));
}

#[tokio::test]
async fn a_queue_without_native_output_reports_its_capability_and_rejects_resume() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let inspected = player.inspect(&f.actor).await.unwrap();
    assert!(!inspected.native);
    let attached = player.claim(&f.actor, inspected.epoch).await.unwrap();
    assert!(!attached.native);
    assert!(matches!(
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence: 1,
                    revision: 0,
                    action: PlayerAction::Resume {},
                }
            )
            .await,
        Err(PlayerError::Unsupported)
    ));
    let state = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert!(!state.desired_playing);
    assert!(!state.playing);
    assert_eq!(state.revision, 0);
    owner.close().await.unwrap();
}

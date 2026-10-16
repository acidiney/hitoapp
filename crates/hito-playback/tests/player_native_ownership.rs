#![cfg(feature = "device")]
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use std::time::Duration;
use uuid::Uuid;

async fn playing(f: &fixture::LocalPlayback, player: &Player) -> hito_playback::PlayerAttachment {
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let queued = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
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
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 2,
                revision: queued.revision,
                action: PlayerAction::Resume {},
            },
        )
        .await
        .unwrap();
    let position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    let mut position = position;
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|state| state.position_ms > 50),
    )
    .await
    .unwrap()
    .unwrap();
    attached
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn another_login_claim_stops_old_output_and_closes_old_private_observations() {
    let f = fixture::LocalPlayback::start().await;
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
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
    let attached = playing(&f, &player).await;
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    let state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    let seen = player.inspect(&another).await.unwrap();
    assert!(seen.state.is_none());
    let next = player.claim(&another, seen.epoch).await.unwrap();
    assert_eq!(next.state.queue_len, 0);
    assert!(next.state.current.is_none());
    assert!(state.has_changed().is_err());
    assert!(position.has_changed().is_err());
    assert!(matches!(
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence: 3,
                    revision: 1,
                    action: PlayerAction::Resume {},
                }
            )
            .await,
        Err(PlayerError::Conflict)
    ));
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    assert!(
        !player
            .inspect(&another)
            .await
            .unwrap()
            .state
            .unwrap()
            .playing
    );
    owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn logout_without_another_command_stops_output_and_closes_private_observations() {
    let f = fixture::LocalPlayback::start().await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
    let attached = playing(&f, &player).await;
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    f.services.identity.logout(&f.token).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.changed().await.is_ok() {}
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while position.changed().await.is_ok() {}
    })
    .await
    .unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    assert!(matches!(
        player.inspect(&f.actor).await,
        Err(PlayerError::Unauthorized)
    ));
    owner.close().await.unwrap();
}

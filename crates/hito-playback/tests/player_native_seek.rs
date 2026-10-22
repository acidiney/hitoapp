#![cfg(feature = "device")]
use hito_core::test_support::playback as fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn paused_seeks_apply_forward_backward_and_zero_without_changing_queue_order() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::native(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    assert_eq!(
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence: 1,
                    revision: 0,
                    action: PlayerAction::Seek { position_ms: 0 },
                }
            )
            .await,
        Err(PlayerError::Invalid)
    );
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
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.phase == PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
    let current = state.borrow().current;
    for (index, position_ms) in [600, 200, 0, 2000, 0].into_iter().enumerate() {
        let command = PlayerCommand {
            id: Uuid::new_v4(),
            lease: attached.lease,
            sequence: index as u64 + 2,
            revision: queued.revision,
            action: PlayerAction::Seek { position_ms },
        };
        let receipt = player.submit(&f.actor, command.clone()).await.unwrap();
        assert_eq!(receipt.revision, queued.revision);
        tokio::time::timeout(
            Duration::from_secs(3),
            position.wait_for(|position| position.seek_id == Some(command.id)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(position.borrow().position_ms, position_ms.min(1000));
        assert!(!state.borrow().desired_playing);
        assert!(!state.borrow().playing);
        assert_eq!(state.borrow().current, current);
        assert_eq!(state.borrow().selection, 1);
        assert_eq!(player.submit(&f.actor, command).await.unwrap(), receipt);
    }
    owner.close().await.unwrap();
}

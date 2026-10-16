#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use uuid::Uuid;

#[tokio::test]
async fn unsupported_and_invalid_controls_do_not_advance_command_or_queue_order() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    for (action, expected) in [
        (
            PlayerAction::Seek { position_ms: 0 },
            PlayerError::Unsupported,
        ),
        (PlayerAction::Volume { level: 0 }, PlayerError::Unsupported),
        (PlayerAction::Previous {}, PlayerError::Unsupported),
        (PlayerAction::Repeat {}, PlayerError::Unsupported),
        (
            PlayerAction::Seek {
                position_ms: u64::MAX,
            },
            PlayerError::Invalid,
        ),
        (PlayerAction::Volume { level: 1001 }, PlayerError::Invalid),
    ] {
        assert_eq!(
            player
                .submit(
                    &f.actor,
                    PlayerCommand {
                        id: Uuid::new_v4(),
                        lease: attached.lease,
                        sequence: 1,
                        revision: 0,
                        action,
                    }
                )
                .await,
            Err(expected)
        );
    }
    let accepted = player
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
    assert_eq!(accepted.revision, 1);
    let state = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(state.volume, 1000);
    assert!(!state.seeking);
    owner.close().await.unwrap();
}

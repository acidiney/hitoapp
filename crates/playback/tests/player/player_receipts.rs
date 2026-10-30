use hito_core::test_support::playback as fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use uuid::Uuid;

#[tokio::test]
async fn retrying_an_accepted_command_replays_its_receipt_without_replacing_queue_entries() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let attachment = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let command = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attachment.lease,
        sequence: 1,
        revision: 0,
        action: PlayerAction::Replace {
            recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
            start: 0,
        },
    };
    let receipt = player.submit(&f.actor, command.clone()).await.unwrap();
    let entry = player
        .inspect(&f.actor)
        .await
        .unwrap()
        .state
        .unwrap()
        .current
        .unwrap()
        .id;
    assert_eq!(
        player.submit(&f.actor, command.clone()).await.unwrap(),
        receipt
    );
    assert_eq!(
        player
            .inspect(&f.actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .current
            .unwrap()
            .id,
        entry
    );
    let mut different = command;
    different.sequence = 2;
    different.revision = 1;
    assert!(matches!(
        player.submit(&f.actor, different).await,
        Err(hito_playback::PlayerError::Conflict)
    ));
    owner.close().await.unwrap();
}

#[tokio::test]
async fn receipt_replay_is_bounded_and_cannot_cross_a_reattachment() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let first = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attached.lease,
        sequence: 1,
        revision: 0,
        action: PlayerAction::Clear {},
    };
    player.submit(&f.actor, first.clone()).await.unwrap();
    let mut last = first.clone();
    for sequence in 2..=65 {
        last = PlayerCommand {
            id: Uuid::new_v4(),
            lease: attached.lease,
            sequence,
            revision: sequence - 1,
            action: PlayerAction::Clear {},
        };
        player.submit(&f.actor, last.clone()).await.unwrap();
    }
    assert!(matches!(
        player.submit(&f.actor, first).await,
        Err(PlayerError::Conflict)
    ));
    assert_eq!(
        player
            .submit(&f.actor, last.clone())
            .await
            .unwrap()
            .revision,
        65
    );
    let next = player.claim(&f.actor, attached.epoch).await.unwrap();
    assert!(matches!(
        player.submit(&f.actor, last.clone()).await,
        Err(PlayerError::Conflict)
    ));
    last.lease = next.lease;
    last.sequence = 1;
    last.revision = 65;
    assert_eq!(player.submit(&f.actor, last).await.unwrap().revision, 66);
    owner.close().await.unwrap();
}

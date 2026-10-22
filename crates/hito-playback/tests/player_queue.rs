use hito_core::test_support::playback as fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use uuid::Uuid;

#[tokio::test]
async fn a_queue_preserves_duplicate_occurrences_and_same_session_reattachment() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let attachment = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let recording = Uuid::parse_str(&f.recording).unwrap();
    let receipt = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attachment.lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![recording, recording],
                    start: 1,
                },
            },
        )
        .await
        .unwrap();
    let page = player
        .queue(&f.actor, attachment.lease, receipt.revision, 0, 20)
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    assert_ne!(page.entries[0].id, page.entries[1].id);
    assert_eq!(page.entries[0].recording.id, f.recording);
    assert_eq!(page.entries[1].recording.id, f.recording);
    let seen = player.inspect(&f.actor).await.unwrap();
    assert_eq!(seen.state.unwrap().current.unwrap().id, page.entries[1].id);
    let attached = player.claim(&f.actor, seen.epoch).await.unwrap();
    let next = player
        .queue(&f.actor, attached.lease, receipt.revision, 0, 20)
        .await
        .unwrap();
    assert_eq!(next.entries[0].id, page.entries[0].id);
    assert_eq!(next.entries[1].id, page.entries[1].id);
    assert_eq!(attached.state.current.unwrap().id, next.entries[1].id);
    owner.close().await.unwrap();
}

#[tokio::test]
async fn relative_navigation_rejects_stale_revisions_and_delayed_sequences_and_never_repeats_on_retry()
 {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    let recording = Uuid::parse_str(&f.recording).unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 1,
                revision: 0,
                action: PlayerAction::Replace {
                    recordings: vec![recording; 3],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    let entries = player
        .queue(&f.actor, lease, 1, 0, 10)
        .await
        .unwrap()
        .entries;
    let next = PlayerCommand {
        id: Uuid::new_v4(),
        lease,
        sequence: 2,
        revision: 1,
        action: PlayerAction::Next {},
    };
    let accepted = player.submit(&f.actor, next.clone()).await.unwrap();
    assert_eq!(player.submit(&f.actor, next).await.unwrap(), accepted);
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
        entries[1].id
    );
    for (sequence, revision) in [(1, 2), (3, 1)] {
        assert!(matches!(
            player
                .submit(
                    &f.actor,
                    PlayerCommand {
                        id: Uuid::new_v4(),
                        lease,
                        sequence,
                        revision,
                        action: PlayerAction::Next {},
                    }
                )
                .await,
            Err(hito_playback::PlayerError::Conflict)
        ));
    }
    assert!(matches!(
        player.queue(&f.actor, lease, 1, 0, 10).await,
        Err(hito_playback::PlayerError::Conflict)
    ));
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 3,
                revision: 2,
                action: PlayerAction::Next {},
            },
        )
        .await
        .unwrap();
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
        entries[2].id
    );
    owner.close().await.unwrap();
}

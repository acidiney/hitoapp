#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use uuid::Uuid;

#[tokio::test]
async fn append_preserves_existing_entries_and_selection_while_select_and_clear_change_it() {
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
                    recordings: vec![recording],
                    start: 0,
                },
            },
        )
        .await
        .unwrap();
    let first = player.inspect(&f.actor).await.unwrap().state.unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 2,
                revision: 1,
                action: PlayerAction::Append {
                    recordings: vec![recording; 2],
                },
            },
        )
        .await
        .unwrap();
    let appended = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(appended.current.unwrap().id, first.current.unwrap().id);
    assert_eq!(appended.selection, first.selection);
    let last = player
        .queue(&f.actor, lease, 2, 2, 1)
        .await
        .unwrap()
        .entries[0]
        .id;
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 3,
                revision: 2,
                action: PlayerAction::Select { entry: last },
            },
        )
        .await
        .unwrap();
    let selected = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(selected.current.unwrap().id, last);
    assert_eq!(selected.selection, first.selection + 1);
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 4,
                revision: 3,
                action: PlayerAction::Clear {},
            },
        )
        .await
        .unwrap();
    let cleared = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(cleared.queue_len, 0);
    assert!(cleared.current.is_none());
    assert_eq!(cleared.selection, selected.selection + 1);
    assert!(
        player
            .queue(&f.actor, lease, 4, 0, 20)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    owner.close().await.unwrap();
}

#[tokio::test]
async fn a_large_queue_rejects_oversized_batches_pages_and_total_capacity() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::start(f.services.clone());
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    let recording = Uuid::parse_str(&f.recording).unwrap();
    for batch in 0..100u64 {
        let recordings = vec![recording; 1000];
        let action = if batch == 0 {
            PlayerAction::Replace {
                recordings,
                start: 0,
            }
        } else {
            PlayerAction::Append { recordings }
        };
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease,
                    sequence: batch + 1,
                    revision: batch,
                    action,
                },
            )
            .await
            .unwrap();
    }
    let state = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(state.queue_len, 100_000);
    assert_eq!(state.selection, 1);
    let first = player.queue(&f.actor, lease, 100, 0, 1000).await.unwrap();
    let last = player
        .queue(&f.actor, lease, 100, 99_000, 1000)
        .await
        .unwrap();
    assert_eq!(first.entries.len(), 1000);
    assert_eq!(last.entries.len(), 1000);
    assert_eq!(last.total, 100_000);
    let ids = first
        .entries
        .iter()
        .chain(&last.entries)
        .map(|entry| entry.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 2000);
    for action in [
        PlayerAction::Append {
            recordings: vec![recording],
        },
        PlayerAction::Replace {
            recordings: vec![recording; 1001],
            start: 0,
        },
    ] {
        assert!(matches!(
            player
                .submit(
                    &f.actor,
                    PlayerCommand {
                        id: Uuid::new_v4(),
                        lease,
                        sequence: 101,
                        revision: 100,
                        action
                    }
                )
                .await,
            Err(PlayerError::Invalid)
        ));
    }
    for (offset, limit) in [(0, 0), (0, 1001), (100_001, 1)] {
        assert!(matches!(
            player.queue(&f.actor, lease, 100, offset, limit).await,
            Err(PlayerError::Invalid)
        ));
    }
    assert!(
        player
            .queue(&f.actor, lease, 100, 100_000, 1)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    assert_eq!(
        player
            .inspect(&f.actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .revision,
        100
    );
    owner.close().await.unwrap();
}

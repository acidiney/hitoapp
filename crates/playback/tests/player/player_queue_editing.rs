use hito_core::test_support::playback as fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use uuid::Uuid;

#[tokio::test]
async fn moving_keeps_the_current_selection_and_removing_it_selects_the_successor() {
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
                    start: 1,
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
    let before = player.inspect(&f.actor).await.unwrap().state.unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 2,
                revision: 1,
                action: PlayerAction::Move {
                    entry: entries[0].id,
                    before: None,
                },
            },
        )
        .await
        .unwrap();
    let moved = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(moved.current.unwrap().id, entries[1].id);
    assert_eq!(moved.selection, before.selection);
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence: 3,
                revision: 2,
                action: PlayerAction::Remove {
                    entry: entries[1].id,
                },
            },
        )
        .await
        .unwrap();
    let removed = player.inspect(&f.actor).await.unwrap().state.unwrap();
    assert_eq!(removed.queue_len, 2);
    assert_eq!(removed.current.unwrap().id, entries[2].id);
    assert_eq!(removed.selection, before.selection + 1);
    assert!(matches!(
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease,
                    sequence: 4,
                    revision: 3,
                    action: PlayerAction::Move {
                        entry: entries[2].id,
                        before: Some(Uuid::new_v4())
                    },
                }
            )
            .await,
        Err(PlayerError::NotFound)
    ));
    let page = player.queue(&f.actor, lease, 3, 0, 10).await.unwrap();
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![entries[2].id, entries[0].id]
    );
    owner.close().await.unwrap();
}

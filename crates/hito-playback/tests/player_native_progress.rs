#![cfg(feature = "device")]
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn pause_and_zero_seek_persist_observed_progress_and_close_waits_for_the_final_checkpoint() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::native(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    for (sequence, revision, action) in [
        (
            1,
            0,
            PlayerAction::Replace {
                recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                start: 0,
            },
        ),
        (2, 1, PlayerAction::Resume {}),
    ] {
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence,
                    revision,
                    action,
                },
            )
            .await
            .unwrap();
    }
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| position.position_ms >= 150),
    )
    .await
    .unwrap()
    .unwrap();
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 3,
                revision: 1,
                action: PlayerAction::Pause {},
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state.phase == PlayerPhase::Ready && !state.playing && !state.desired_playing
        }),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let activity = f.services.collection.activity(&f.actor).await.unwrap();
            if activity
                .history
                .iter()
                .any(|entry| entry.id == f.recording && entry.position_ms >= 150)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let seek = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 4,
                revision: 1,
                action: PlayerAction::Seek { position_ms: 0 },
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| position.seek_id == Some(seek.id)),
    )
    .await
    .unwrap()
    .unwrap();
    owner.close().await.unwrap();
    let activity = f.services.collection.activity(&f.actor).await.unwrap();
    assert_eq!(activity.history.len(), 1);
    assert_eq!(activity.history[0].id, f.recording);
    assert_eq!(activity.history[0].position_ms, 0);
    assert_eq!(activity.history[0].speed, 1.0);
}

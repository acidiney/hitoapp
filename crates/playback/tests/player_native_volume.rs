#![cfg(feature = "device")]
use hito_audio::test_support::capture;
#[path = "fixtures/recovery.rs"]
mod fixture;
use hito_core::test_support::support;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn volume_is_applied_before_start_and_survives_same_session_reattachment() {
    let f = fixture::Recovery::start().await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    assert_eq!(attached.state.volume, 1000);
    for (index, revision, action) in [
        (1, 0, PlayerAction::Volume { level: 0 }),
        (
            2,
            0,
            PlayerAction::Replace {
                recordings: vec![Uuid::parse_str(&f.recording).unwrap()],
                start: 0,
            },
        ),
        (3, 1, PlayerAction::Resume {}),
    ] {
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence: index,
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
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| position.position_ms > 100),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.15);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 4,
                revision: 1,
                action: PlayerAction::Volume { level: 250 },
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            capture.after(capture.mark(), 0.1);
            let amplitude = capture::amplitude(&capture.recent(0.05), 440.);
            if (0.07..0.13).contains(&amplitude) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let renewed = player.claim(&f.actor, attached.epoch).await.unwrap();
    assert_eq!(renewed.state.volume, 250);
    assert_eq!(renewed.state.revision, 1);
    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: renewed.lease,
                sequence: 1,
                revision: 1,
                action: PlayerAction::Volume { level: 1000 },
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            capture.after(capture.mark(), 0.1);
            if capture::amplitude(&capture.recent(0.05), 440.) > 0.3 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    owner.close().await.unwrap();
}

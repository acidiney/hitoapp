#![cfg(feature = "device")]
use hito_audio::test_support::{capture, restrict_output_rates};
use hito_core::test_support::playback as fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use std::time::Duration;
use uuid::Uuid;

async fn play(
    player: &Player,
    f: &fixture::LocalPlayback,
    lease: Uuid,
    sequence: u64,
    revision: u64,
) {
    let queued = player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease,
                sequence,
                revision,
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
                lease,
                sequence: sequence + 1,
                revision: queued.revision,
                action: PlayerAction::Resume {},
            },
        )
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_device_that_cannot_play_the_source_reports_a_distinct_output_format_error() {
    let f = fixture::LocalPlayback::start().await;
    let (player, owner) = Player::native(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    {
        let _device = restrict_output_rates(&[]);
        play(&player, &f, attached.lease, 1, 0).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let state = player.inspect(&f.actor).await.unwrap().state.unwrap();
                if let Some(failure) = state.last_failure {
                    assert_eq!(failure.error, PlayerError::OutputFormat);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
    }
    let revision = player
        .inspect(&f.actor)
        .await
        .unwrap()
        .state
        .unwrap()
        .revision;
    let capture = capture::Capture::open();
    play(&player, &f, attached.lease, 3, revision).await;
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(4),
        position.wait_for(|state| state.position_ms > 100),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.1)) > 0.00001);
    assert_eq!(
        player.inspect(&f.actor).await.unwrap().state.unwrap().error,
        None
    );
    owner.close().await.unwrap();
}

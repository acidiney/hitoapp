#![cfg(feature = "device")]
use hito_audio::test_support::restrict_output_rates;
use hito_core::test_support::playback as fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_device_that_cannot_play_the_source_reports_a_distinct_output_format_error() {
    let f = fixture::LocalPlayback::start().await;
    let _device = restrict_output_rates(&[]);
    let (player, owner) = Player::native(f.services.clone());
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
    owner.close().await.unwrap();
}

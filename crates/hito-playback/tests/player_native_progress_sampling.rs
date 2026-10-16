#![cfg(feature = "device")]
#[path = "fixtures/recovery.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_core::Action;
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn continuous_playback_coalesces_position_ticks_and_saves_periodic_progress() {
    let f = fixture::Recovery::start().await;
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
    let saved = tokio::time::timeout(Duration::from_secs(7), async {
        loop {
            let activity = f.services.collection.activity(&f.actor).await.unwrap();
            if let Some(entry) = activity.history.first()
                && entry.position_ms >= 4_000
            {
                break entry.position_ms;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        player
            .inspect(&f.actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .playing
    );
    let journal = f.services.collection.events(&f.actor, 0).await.unwrap();
    assert!((2..=3).contains(&journal.operations.len()));
    assert!(!journal.has_more);
    assert!(journal.operations.iter().all(|event| matches!(
        event.operation.action,
        Action::ProgressSet { recording_id, speed: 1.0, .. }
            if recording_id.to_string() == f.recording
    )));
    owner.close().await.unwrap();
    let activity = f.services.collection.activity(&f.actor).await.unwrap();
    assert_eq!(activity.history.len(), 1);
    assert!(activity.history[0].position_ms >= saved);
}

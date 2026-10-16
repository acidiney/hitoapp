#![cfg(feature = "device")]
#[path = "../../hito-core/tests/fixtures/playback.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_core::{Action, Search};
use hito_playback::{Player, PlayerAction, PlayerCommand};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn skipping_a_recording_saves_its_position_before_the_next_recording_finishes() {
    let f = fixture::LocalPlayback::start().await;
    support::write_wav(&f.music.join("next.wav"), 7);
    support::scan(&f.services, &f.actor, &f.library).await;
    let next = f
        .services
        .catalog
        .recordings(&f.actor, Search::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|recording| recording.id != f.recording)
        .unwrap();
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
                recordings: vec![
                    Uuid::parse_str(&f.recording).unwrap(),
                    Uuid::parse_str(&next.id).unwrap(),
                ],
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
                action: PlayerAction::Next {},
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(4),
        state.wait_for(|state| state.current.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    owner.close().await.unwrap();
    let activity = f.services.collection.activity(&f.actor).await.unwrap();
    assert_eq!(activity.history.len(), 2);
    let first = activity
        .history
        .iter()
        .find(|entry| entry.id == f.recording)
        .unwrap();
    assert!((150..1_000).contains(&first.position_ms));
    assert_eq!(
        activity
            .history
            .iter()
            .find(|entry| entry.id == next.id)
            .unwrap()
            .position_ms,
        1_000
    );
    let journal = f.services.collection.events(&f.actor, 0).await.unwrap();
    let mut next_seen = false;
    for event in journal.operations {
        let Action::ProgressSet { recording_id, .. } = event.operation.action else {
            panic!("the native client wrote a non-progress operation");
        };
        if recording_id.to_string() == next.id {
            next_seen = true;
        } else {
            assert_eq!(recording_id.to_string(), f.recording);
            assert!(!next_seen);
        }
    }
    assert!(next_seen);
}

#![cfg(feature = "device")]
#[path = "fixtures/authority_outage.rs"]
mod authority_outage;
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
#[path = "fixtures/recovery.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn temporary_authority_failure_silences_output_and_resumes_the_same_entry_near_its_position()
{
    let f = fixture::Recovery::start().await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    for (index, action) in [
        PlayerAction::Replace {
            recordings: vec![Uuid::parse_str(&f.recording).unwrap(); 2],
            start: 0,
        },
        PlayerAction::Seek { position_ms: 2000 },
        PlayerAction::Volume { level: 250 },
        PlayerAction::Resume {},
    ]
    .into_iter()
    .enumerate()
    {
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence: index as u64 + 1,
                    revision: u64::from(index > 0),
                    action,
                },
            )
            .await
            .unwrap();
    }
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.playing),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| position.position_ms > 2100),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.1);
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    let before = position.borrow().position_ms;
    let selected = state.borrow().current.unwrap();
    let outage = authority_outage::Outage::start(f.directory.path()).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.phase == PlayerPhase::Suspended),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.4);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    assert_eq!(state.borrow().current, Some(selected));
    assert_eq!(state.borrow().queue_len, 2);
    assert!(state.borrow().desired_playing);
    assert!(!state.borrow().playing);
    assert_eq!(state.borrow().volume, 250);
    let pause = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attached.lease,
        sequence: 5,
        revision: 1,
        action: PlayerAction::Pause {},
    };
    assert_eq!(
        player.submit(&f.actor, pause.clone()).await,
        Err(PlayerError::Unavailable)
    );
    outage.restore().await;
    tokio::time::timeout(
        Duration::from_secs(4),
        state.wait_for(|state| state.phase == PlayerPhase::Ready && state.playing),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.2);
    assert!(capture::energy(&capture.recent(0.1)) > 0.00001);
    assert!(position.borrow().position_ms >= before);
    assert_eq!(state.borrow().current, Some(selected));
    assert_eq!(state.borrow().revision, 1);
    assert_eq!(
        player.inspect(&f.actor).await.unwrap().epoch,
        attached.epoch
    );
    player.submit(&f.actor, pause).await.unwrap();
    owner.close().await.unwrap();
    assert!(
        f.services
            .collection
            .activity(&f.actor)
            .await
            .unwrap()
            .history[0]
            .position_ms
            >= before as i64
    );
}

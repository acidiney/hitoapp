#![cfg(feature = "device")]
use hito_audio::test_support::capture;
#[path = "../fixtures/navigation.rs"]
mod fixture;
use hito_core::test_support::support;
use hito_playback::{PlayerAction, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn cleared_and_replaced_queues_cannot_visit_history_from_an_earlier_generation() {
    for clear in [false, true] {
        let mut f = fixture::Navigation::start(2, 0).await;
        f.send(PlayerAction::Resume {}).await;
        f.playing(f.entries[0].id).await;
        f.send(PlayerAction::Next {}).await;
        f.playing(f.entries[1].id).await;
        f.send(PlayerAction::Pause {}).await;
        let recordings = vec![Uuid::parse_str(&f.source.recording).unwrap()];
        if clear {
            f.send(PlayerAction::Clear {}).await;
            let appended = f.send(PlayerAction::Append { recordings }).await;
            let page = f
                .player
                .queue(&f.source.actor, f.lease, appended.revision, 0, 1)
                .await
                .unwrap();
            f.send(PlayerAction::Select {
                entry: page.entries[0].id,
            })
            .await;
        } else {
            f.send(PlayerAction::Replace {
                recordings,
                start: 0,
            })
            .await;
        }
        let before = f.state.borrow().clone();
        let previous = f.send(PlayerAction::Previous {}).await;
        assert_eq!(previous.revision, before.revision);
        assert_eq!(f.state.borrow().current, before.current);
        assert_eq!(f.state.borrow().selection, before.selection);
        assert!(!f.state.borrow().desired_playing);
        tokio::time::timeout(
            Duration::from_secs(3),
            f.position.wait_for(|p| p.seek_id == Some(previous.id)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(f.position.borrow().position_ms, 0);
        f.owner.close().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn previous_remains_available_after_queue_end_and_clear_removes_that_availability() {
    let mut f = fixture::Navigation::start(2, 0).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[0].id).await;
    f.send(PlayerAction::Next {}).await;
    f.playing(f.entries[1].id).await;
    f.send(PlayerAction::Next {}).await;
    assert!(f.state.borrow().current.is_none());
    assert_eq!(
        serde_json::to_value(&*f.state.borrow()).unwrap()["can_previous"],
        true
    );
    f.send(PlayerAction::Previous {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[1].id);
    assert!(!f.state.borrow().desired_playing);
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state
            .wait_for(|s| s.phase == PlayerPhase::Ready && !s.playing),
    )
    .await
    .unwrap()
    .unwrap();
    f.send(PlayerAction::Clear {}).await;
    assert_eq!(
        serde_json::to_value(&*f.state.borrow()).unwrap()["can_previous"],
        false
    );
    f.owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn merely_visiting_paused_entries_does_not_create_history_or_unpause_previous() {
    let mut f = fixture::Navigation::start(4, 0).await;
    let capture = capture::Capture::open();
    f.send(PlayerAction::Select {
        entry: f.entries[1].id,
    })
    .await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[1].id).await;
    f.send(PlayerAction::Pause {}).await;
    for index in [2, 3] {
        f.send(PlayerAction::Select {
            entry: f.entries[index].id,
        })
        .await;
        tokio::time::timeout(
            Duration::from_secs(3),
            f.state
                .wait_for(|s| s.phase == PlayerPhase::Ready && !s.playing),
        )
        .await
        .unwrap()
        .unwrap();
    }
    f.send(PlayerAction::Previous {}).await;
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[1].id);
    assert!(!f.state.borrow().desired_playing);
    let revision = f.state.borrow().revision;
    let restarted = f.send(PlayerAction::Previous {}).await;
    assert_eq!(restarted.revision, revision);
    assert_eq!(f.state.borrow().current.unwrap().id, f.entries[1].id);
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|p| p.seek_id == Some(restarted.id)),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    f.owner.close().await.unwrap();
}

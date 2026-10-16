#![cfg(feature = "device")]
#[path = "fixtures/navigation.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_core::Action;
use hito_playback::{PlayerAction, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn genuine_submillisecond_audio_after_a_saved_start_counts_as_listening() {
    let mut f = fixture::Navigation::short().await;
    let entry = f.entries[0].id;
    f.send(PlayerAction::ResumeSaved {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|s| s.current.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    f.send(PlayerAction::Previous {}).await;
    assert_eq!(f.state.borrow().current.map(|e| e.id), Some(entry));
    f.owner.close().await.unwrap();
    assert_eq!(
        f.source
            .services
            .collection
            .events(&f.source.actor, 0)
            .await
            .unwrap()
            .operations
            .len(),
        1
    );
    assert_eq!(
        f.source
            .services
            .collection
            .saved_position(
                &f.source.actor,
                Uuid::parse_str(&f.source.recording).unwrap()
            )
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn restored_end_and_beyond_end_advance_without_a_checkpoint_or_played_occurrence() {
    for position_ms in [10_000, 15_000] {
        let mut f = fixture::Navigation::start(3, 0).await;
        f.send(PlayerAction::Resume {}).await;
        f.playing(f.entries[0].id).await;
        f.send(PlayerAction::Pause {}).await;
        tokio::time::timeout(
            Duration::from_secs(3),
            f.state
                .wait_for(|s| s.phase == PlayerPhase::Ready && !s.playing),
        )
        .await
        .unwrap()
        .unwrap();
        let paused = f.position.borrow().position_ms;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if f.source
                    .services
                    .collection
                    .saved_position(
                        &f.source.actor,
                        Uuid::parse_str(&f.source.recording).unwrap(),
                    )
                    .await
                    .unwrap()
                    .is_some_and(|s| s.position_ms == paused)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        f.send(PlayerAction::Next {}).await;
        support::synchronize(
            &f.source.services,
            &f.source.actor,
            vec![Action::ProgressSet {
                recording_id: Uuid::parse_str(&f.source.recording).unwrap(),
                position_ms,
                speed: 0.5,
            }],
        )
        .await;
        let before = f
            .source
            .services
            .collection
            .events(&f.source.actor, 0)
            .await
            .unwrap()
            .cursor;
        f.send(PlayerAction::ResumeSaved {}).await;
        tokio::time::timeout(
            Duration::from_secs(3),
            f.state
                .wait_for(|s| s.current.is_some_and(|e| e.id == f.entries[2].id)),
        )
        .await
        .unwrap()
        .unwrap();
        f.send(PlayerAction::Pause {}).await;
        f.send(PlayerAction::Previous {}).await;
        assert_eq!(
            f.state.borrow().current.unwrap().id,
            f.entries[0].id,
            "restored completion must not enter played history"
        );
        f.owner.close().await.unwrap();
        let journal = f
            .source
            .services
            .collection
            .events(&f.source.actor, before)
            .await
            .unwrap();
        assert!(journal.operations.iter().all(|e| !matches!(e.operation.action, Action::ProgressSet { position_ms, .. } if position_ms >= 10_000)), "restored completion must not manufacture listening progress");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn source_recovery_retains_the_resolved_cursor_instead_of_loading_newer_saved_data() {
    let mut f = fixture::Navigation::split().await;
    let recording = Uuid::parse_str(&f.source.recording).unwrap();
    support::synchronize(
        &f.source.services,
        &f.source.actor,
        vec![Action::ProgressSet {
            recording_id: recording,
            position_ms: 6000,
            speed: 0.5,
        }],
    )
    .await;
    f.send(PlayerAction::ResumeSaved {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|p| p.position_ms > 6200),
    )
    .await
    .unwrap()
    .unwrap();
    f.send(PlayerAction::Pause {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state
            .wait_for(|s| s.phase == PlayerPhase::Ready && !s.playing),
    )
    .await
    .unwrap()
    .unwrap();
    let original = f.state.borrow().source_id.clone().unwrap();
    let before = f.position.borrow().position_ms;
    support::synchronize(
        &f.source.services,
        &f.source.actor,
        vec![Action::ProgressSet {
            recording_id: recording,
            position_ms: 1000,
            speed: 2.,
        }],
    )
    .await;
    f.source
        .services
        .storage
        .remove_folder(&f.source.actor, &f.source.first.id, f.source.first.revision)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|s| {
            s.phase == PlayerPhase::Ready && s.source_id.as_ref().is_some_and(|id| id != &original)
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!f.state.borrow().playing);
    assert!(f.position.borrow().position_ms >= before);
    assert_eq!(f.position.borrow().seek_id, None);
    f.owner.close().await.unwrap();
}

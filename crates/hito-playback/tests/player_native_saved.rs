#![cfg(feature = "device")]
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
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
async fn saved_start_loads_current_core_position_before_the_first_presented_audio() {
    let mut f = fixture::Navigation::split().await;
    let recording = Uuid::parse_str(&f.source.recording).unwrap();
    support::synchronize(
        &f.source.services,
        &f.source.actor,
        vec![Action::ProgressSet {
            recording_id: recording,
            position_ms: 1000,
            speed: 0.5,
        }],
    )
    .await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|s| s.phase == PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
    f.send(PlayerAction::Speed { rate: 1500 }).await;
    support::synchronize(
        &f.source.services,
        &f.source.actor,
        vec![Action::ProgressSet {
            recording_id: recording,
            position_ms: 6000,
            speed: 2.,
        }],
    )
    .await;
    let capture = capture::Capture::open();
    let mark = capture.mark();
    f.send(
        serde_json::from_str::<PlayerAction>(r#"{"kind":"resume_saved"}"#)
            .expect("saved start is an explicit public intent"),
    )
    .await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|p| p.position_ms >= 6000),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(f.position.borrow().seek_id, None);
    assert_eq!(f.state.borrow().speed, 1500);
    capture.after(mark, 0.4);
    let samples = capture.all();
    assert!(capture::amplitude(&samples[mark..], 880.) > 0.02);
    assert!(capture::amplitude(&samples[mark..], 440.) < 0.005);
    f.send(PlayerAction::Pause {}).await;
    f.owner.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn saved_start_waits_for_earlier_writes_and_pause_preserves_the_loaded_cursor_without_activity() {
    let mut f = fixture::Navigation::split().await;
    let recording = Uuid::parse_str(&f.source.recording).unwrap();
    support::synchronize(&f.source.services, &f.source.actor, vec![Action::ProgressSet {
        recording_id: recording, position_ms: 1000, speed: 1.,
    }]).await;
    tokio::time::timeout(Duration::from_secs(3), f.state.wait_for(|s| s.phase == PlayerPhase::Ready)).await.unwrap().unwrap();
    let pool = sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(f.source.directory.path().join("hito.db"))).await.unwrap();
    let transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let seek = f.send(PlayerAction::Seek { position_ms: 6000 }).await;
    tokio::time::timeout(Duration::from_secs(3), f.position.wait_for(|p| p.seek_id == Some(seek.id))).await.unwrap().unwrap();
    let capture = capture::Capture::open();
    let command = f.request(serde_json::from_str(r#"{"kind":"resume_saved"}"#).unwrap());
    let receipt = f.submit(command.clone()).await;
    assert_eq!(f.player.submit(&f.source.actor, command).await.unwrap(), receipt);
    f.send(PlayerAction::Pause {}).await;
    capture.after(capture.mark(), 0.3);
    assert_eq!(f.state.borrow().phase, PlayerPhase::Loading);
    assert!(!f.state.borrow().desired_playing);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    assert_eq!(f.source.services.collection.saved_position(&f.source.actor, recording).await.unwrap().unwrap().position_ms, 1000);
    transaction.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), f.state.wait_for(|s| s.phase == PlayerPhase::Ready && !s.playing)).await.unwrap().unwrap();
    assert_eq!(f.position.borrow().position_ms, 6000);
    assert_eq!(f.position.borrow().seek_id, None);
    capture.after(capture.mark(), 0.2);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    f.send(PlayerAction::Next {}).await;
    f.owner.close().await.unwrap();
    let journal = f.source.services.collection.events(&f.source.actor, 0).await.unwrap();
    assert_eq!(journal.operations.len(), 2, "restoring a cursor must not add a checkpoint");
    pool.close().await;
}

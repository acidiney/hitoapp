#![cfg(feature = "device")]
#[path = "../fixtures/authority_outage.rs"]
mod authority_outage;
use hito_audio::test_support::capture;
#[path = "../fixtures/navigation.rs"]
mod fixture;
#[path = "../fixtures/saved_store.rs"]
mod saved_store;
use hito_core::Action;
use hito_core::test_support::support;
use hito_playback::{PlayerAction, PlayerError, PlayerPhase};
use std::time::Duration;
use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Loads {
    started: Arc<AtomicUsize>,
    succeeded: Arc<AtomicUsize>,
    changed: Arc<tokio::sync::Notify>,
}
impl Loads {
    async fn successful(&self) {
        loop {
            let changed = self.changed.notified();
            if self.succeeded.load(Ordering::SeqCst) != 0 {
                return;
            }
            changed.await;
        }
    }
}
impl Write for Loads {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Ok(event) = serde_json::from_slice::<serde_json::Value>(bytes) {
            if event["fields"]["event"] == "player_saved_load_started" {
                self.started.fetch_add(1, Ordering::SeqCst);
            }
            if event["fields"]["event"] == "player_saved_load_finished"
                && event["fields"]["success"] == true
            {
                self.succeeded.fetch_add(1, Ordering::SeqCst);
                self.changed.notify_one();
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_successful_read_during_suspension_restores_once_then_preserves_pause_progress_and_recovery()
 {
    let loads = Loads::default();
    let writer = loads.clone();
    hito_testkit::logs::anchor_callsite_interest();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(move || writer.clone())
            .finish(),
    );
    let mut f = fixture::Navigation::split().with_subscriber(dispatch).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|s| s.phase == PlayerPhase::Ready),
    )
    .await
    .unwrap()
    .unwrap();
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
    f.send(PlayerAction::Speed { rate: 1500 }).await;
    let capture = capture::Capture::open();
    let store =
        saved_store::SavedStore::delayed_iterations(f.source.directory.path(), 2_000_000).await;
    f.send(PlayerAction::ResumeSaved {}).await;
    f.send(PlayerAction::Pause {}).await;
    store.reading().await;
    let outage = authority_outage::Outage::recordings(f.source.directory.path()).await;
    assert!(matches!(
        f.player.queue(&f.source.actor, f.lease, 1, 0, 1).await,
        Err(PlayerError::Unavailable)
    ));
    tokio::time::timeout(Duration::from_secs(3), loads.successful())
        .await
        .unwrap();
    let suspended = f
        .player
        .inspect(&f.source.actor)
        .await
        .unwrap()
        .state
        .unwrap();
    assert_eq!(suspended.phase, PlayerPhase::Suspended);
    assert_eq!(suspended.error, Some(PlayerError::Unavailable));
    capture.after(capture.mark(), 0.15);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    store.restore().await;
    outage.restore().await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state
            .wait_for(|s| s.phase == PlayerPhase::Ready && !s.playing),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(f.position.borrow().position_ms, 6000);
    assert_eq!(f.position.borrow().seek_id, None);
    assert!(!f.state.borrow().desired_playing);
    assert_eq!(f.state.borrow().speed, 1500);
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
    let mark = capture.mark();
    f.send(PlayerAction::Resume {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|p| p.position_ms >= 6200),
    )
    .await
    .unwrap()
    .unwrap();
    capture.after(mark, 0.3);
    let samples = capture.all();
    assert!(capture::amplitude(&samples[mark..], 880.) > 0.02);
    assert!(capture::amplitude(&samples[mark..], 440.) < 0.005);
    f.send(PlayerAction::Pause {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state
            .wait_for(|s| s.phase == PlayerPhase::Ready && !s.playing),
    )
    .await
    .unwrap()
    .unwrap();
    let before = f.position.borrow().position_ms;
    let source = f.state.borrow().source_id.clone().unwrap();
    f.source
        .services
        .storage
        .remove_folder(&f.source.actor, &f.source.first.id, f.source.first.revision)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|s| {
            s.phase == PlayerPhase::Ready && s.source_id.as_ref().is_some_and(|id| id != &source)
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!f.state.borrow().playing);
    assert!(f.position.borrow().position_ms >= before);
    f.send(PlayerAction::Resume {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|p| p.position_ms >= before + 100),
    )
    .await
    .unwrap()
    .unwrap();
    f.owner.close().await.unwrap();
    assert_eq!(loads.started.load(Ordering::SeqCst), 1);
    assert_eq!(loads.succeeded.load(Ordering::SeqCst), 1);
    let activity = f
        .source
        .services
        .collection
        .activity(&f.source.actor)
        .await
        .unwrap();
    assert!(activity.history[0].position_ms >= (before + 100) as i64);
    assert_eq!(activity.history[0].speed, 1.5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_saved_read_finishing_during_suspension_preserves_the_access_message_until_authority_recovers()
 {
    let mut f = fixture::Navigation::start(1, 0).await;
    support::synchronize(
        &f.source.services,
        &f.source.actor,
        vec![Action::ProgressSet {
            recording_id: Uuid::parse_str(&f.source.recording).unwrap(),
            position_ms: 6000,
            speed: 1.,
        }],
    )
    .await;
    let store = saved_store::SavedStore::delayed(f.source.directory.path()).await;
    f.send(PlayerAction::ResumeSaved {}).await;
    let outage = authority_outage::Outage::start(f.source.directory.path()).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state.wait_for(|s| s.phase == PlayerPhase::Suspended),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(f.state.borrow().phase, PlayerPhase::Suspended);
    assert_eq!(f.state.borrow().error, Some(PlayerError::Unavailable));
    assert!(!f.state.borrow().playing);
    store.restore().await;
    outage.restore().await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.state
            .wait_for(|s| s.phase == PlayerPhase::Failed && s.error == Some(PlayerError::Resume)),
    )
    .await
    .unwrap()
    .unwrap();
    f.send(PlayerAction::ResumeSaved {}).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        f.position.wait_for(|p| p.position_ms > 6100),
    )
    .await
    .unwrap()
    .unwrap();
    f.owner.close().await.unwrap();
}

#![cfg(feature = "device")]
#[path = "fixtures/authority_outage.rs"]
mod authority_outage;
#[path = "fixtures/navigation.rs"]
mod fixture;
#[path = "fixtures/saved_store.rs"]
mod saved_store;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_core::Action;
use hito_playback::{PlayerAction, PlayerError, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

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

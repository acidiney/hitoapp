#![cfg(feature = "device")]
#[path = "fixtures/navigation.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use hito_playback::PlayerAction;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn speed_is_applied_before_playback_and_saved_with_source_time_progress() {
    let mut f = fixture::Navigation::start(1, 0).await;
    let command = serde_json::from_str::<PlayerAction>(r#"{"kind":"speed","rate":2000}"#)
        .expect("native speed is a public command");
    f.send(command).await;
    f.send(PlayerAction::Resume {}).await;
    f.playing(f.entries[0].id).await;
    f.send(PlayerAction::Pause {}).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let activity = f.source.services.collection.activity(&f.source.actor).await.unwrap();
            if activity.history.iter().any(|item| item.id == f.source.recording && item.speed == 2. && item.position_ms > 100) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    f.owner.close().await.unwrap();
}

#![cfg(feature = "device")]
#[path = "fixtures/navigation.rs"]
mod fixture;
use hito_core::test_support::support;
use hito_playback::PlayerAction;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn final_selection_anchors_a_batched_replacement_before_its_first_shuffle_next() {
    let mut f = fixture::Navigation::start(1000, 0).await;
    f.send(PlayerAction::Shuffle {}).await;
    f.send(PlayerAction::Append {
        recordings: vec![f.entries[0].recording.id.parse().unwrap()],
    })
    .await;
    let revision = f.state.borrow().revision;
    let start = f
        .player
        .queue(&f.source.actor, f.lease, revision, 1000, 1)
        .await
        .unwrap()
        .entries
        .remove(0)
        .id;
    f.send(PlayerAction::Select { entry: start }).await;
    f.send(PlayerAction::Next {}).await;
    let selected = f.state.borrow().current;
    f.owner.close().await.unwrap();
    assert!(
        selected.is_some(),
        "The first Next skipped the entire first replacement batch"
    );
    assert!(
        f.entries
            .iter()
            .any(|entry| Some(entry.id) == selected.map(|entry| entry.id))
    );
}

#[path = "fixtures/playback.rs"]
mod fixture;
mod support;

use hito_core::Action;
use uuid::Uuid;

#[tokio::test]
async fn saved_position_distinguishes_missing_zero_and_current_progress() {
    let f = fixture::LocalPlayback::start().await;
    let recording = Uuid::parse_str(&f.recording).unwrap();
    assert_eq!(
        f.services
            .collection
            .saved_position(&f.actor, recording)
            .await
            .unwrap(),
        None
    );
    for position_ms in [0, 700, 50] {
        support::synchronize(
            &f.services,
            &f.actor,
            vec![Action::ProgressSet {
                recording_id: recording,
                position_ms,
                speed: 2.5,
            }],
        )
        .await;
        let saved = f
            .services
            .collection
            .saved_position(&f.actor, recording)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.position_ms, position_ms as u64);
    }
}

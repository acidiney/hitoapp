#![cfg(feature = "device")]
use hito_audio::test_support as audio;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn ready_without_presented_progress_cannot_reset_repeat_failure_exclusions() {
    let f = audio_fixture::AudioLibrary::start().await;
    let mut wave = audio::wave(48_000, 1, 48_000, |_, _| 1000);
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut wave, &mut bytes).unwrap();
    f.server
        .audio(axum::http::StatusCode::OK, audio_fixture::headers(), bytes)
        .await;
    let tail = f.server.hold_broken_tail(4096).await;
    let (player, owner) = Player::native(f.hito.clone());
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    for (sequence, action) in [
        PlayerAction::Replace {
            recordings: vec![f.source.recording_id.parse().unwrap(); 2],
            start: 0,
        },
        PlayerAction::Repeat {},
        PlayerAction::Resume {},
    ]
    .into_iter()
    .enumerate()
    {
        let revision = state.borrow().revision;
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence: sequence as u64 + 1,
                    revision,
                    action,
                },
            )
            .await
            .unwrap();
    }
    let mut starts = Vec::new();
    for attempt in 0..4 {
        tokio::time::timeout(Duration::from_secs(3), tail.entered())
            .await
            .unwrap();
        let ready = tokio::time::timeout(
            Duration::from_secs(3),
            state.wait_for(|s| {
                s.phase == PlayerPhase::Ready && s.playing && s.selection == 1 + attempt / 2
            }),
        )
        .await
        .is_ok_and(|result| result.is_ok());
        if !ready {
            let snapshot = state.borrow().clone();
            let cursor = *position.borrow();
            let requests = f.server.requests("v1/audio").await.len();
            owner.close().await.unwrap();
            panic!(
                "Attempt {attempt} never became Ready: {snapshot:?}, {cursor:?}, requests={requests}"
            );
        }
        starts.push(position.borrow().position_ms);
        tail.release();
    }
    let ended = tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|s| s.current.is_none()),
    )
    .await
    .is_ok_and(|result| result.is_ok());
    owner.close().await.unwrap();
    assert_eq!(
        starts,
        vec![0; 4],
        "The fixture must fail before presented progress"
    );
    assert!(
        ended,
        "Ready without progress erased an earlier failed occurrence"
    );
    assert_eq!(f.server.requests("v1/audio").await.len(), 4);
}

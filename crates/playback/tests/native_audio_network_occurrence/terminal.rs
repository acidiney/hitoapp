use super::super::fixture;
use super::resources::Resources;
use futures_util::FutureExt;
use hito_audio::test_support::capture;
use hito_audio::test_support::output_witness::OutputWitness;
use hito_core::test_support::hang_guard::hang_guard;
use hito_playback::{Player, PlayerAction, PlayerError, PlayerPhase};
use hito_testkit::logs::LogCapture;
use std::{panic::AssertUnwindSafe, time::Duration};
use tracing::instrument::WithSubscriber;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn audio_403_is_terminal_and_does_not_start_a_logical_retry() {
    let f = fixture::Fixture::start().await;
    f.refuse_audio();
    let private = f.private_values();
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    exercise_terminal(f, false).with_subscriber(dispatch).await;
    logs.assert_excludes(&private.iter().map(String::as_str).collect::<Vec<_>>());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn provider_credentials_failure_preserves_the_native_player_owner() {
    let f = fixture::Fixture::start().await;
    f.credentials_audio();
    let private = f.private_values();
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    exercise_terminal(f, true).with_subscriber(dispatch).await;
    logs.assert_excludes(&private.iter().map(String::as_str).collect::<Vec<_>>());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn invalid_audio_representation_is_terminal_and_does_not_start_a_logical_retry() {
    let f = fixture::Fixture::start().await;
    f.invalid_audio();
    let private = f.private_values();
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    exercise_terminal(f, false).with_subscriber(dispatch).await;
    logs.assert_excludes(&private.iter().map(String::as_str).collect::<Vec<_>>());
}

async fn exercise_terminal(fixture: fixture::Fixture, check_owner: bool) {
    let mut resources = Resources::new(fixture);
    let (actor, recording, sentinel, services, order) = {
        let fixture = resources.fixture.as_ref().unwrap();
        (
            fixture.occurrence.actor.clone(),
            fixture.occurrence.recording,
            fixture.occurrence.sentinel,
            fixture.occurrence.services.clone(),
            fixture.occurrence.order.clone(),
        )
    };
    let behavior = AssertUnwindSafe(async {
        resources.witness = Some(OutputWitness::begin(48_000 * 20, 4096).unwrap());
        resources.capture = Some(capture::Capture::open());
        resources.native = Some(Player::native(services));
        let (player, _) = resources.native.as_ref().unwrap();
        let attached = hang_guard("terminal audio occurrence claim", async {
            player
                .claim(&actor, player.inspect(&actor).await.unwrap().epoch)
                .await
                .unwrap()
        })
        .await;
        let mut state = player.subscribe(&actor, attached.lease).await.unwrap();
        let mut position = player
            .subscribe_position(&actor, attached.lease)
            .await
            .unwrap();
        let queued = player
            .submit(
                &actor,
                super::resources::command(
                    attached.lease,
                    1,
                    0,
                    PlayerAction::Replace {
                        recordings: vec![recording, sentinel],
                        start: 0,
                    },
                ),
            )
            .await
            .unwrap();
        let target_entry = player
            .inspect(&actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .current
            .unwrap()
            .id;
        player
            .submit(
                &actor,
                super::resources::command(
                    attached.lease,
                    2,
                    queued.revision,
                    PlayerAction::Resume {},
                ),
            )
            .await
            .unwrap();
        let selected = tokio::time::timeout(
            Duration::from_secs(5),
            state.wait_for(|snapshot| {
                snapshot.selection == 2
                    && snapshot.current_index == Some(1)
                    && snapshot.playing
                    && snapshot.phase == PlayerPhase::Ready
            }),
        )
        .await
        .unwrap()
        .unwrap()
        .clone();
        assert_eq!(
            selected.current.as_ref().map(|entry| entry.recording_id),
            Some(sentinel)
        );
        assert!(selected.retrying.is_none());
        assert_eq!(
            selected
                .last_failure
                .as_ref()
                .map(|failure| (failure.entry.id, failure.error)),
            Some((target_entry, PlayerError::Playback))
        );
        assert_eq!(
            resources.fixture.as_ref().unwrap().handler_calls(),
            1,
            "terminal audio must not be reopened"
        );
        assert_eq!(resources.fixture.as_ref().unwrap().audio_requests(), 1);
        assert_eq!(
            resources
                .fixture
                .as_ref()
                .unwrap()
                .occurrence
                .second
                .requests_for("v1/sources")
                .len(),
            1
        );
        assert_eq!(order.lock().unwrap().as_slice(), ['A', 'B']);
        let sentinel_entry = selected.current.as_ref().unwrap().id;
        let heard = *tokio::time::timeout(
            Duration::from_secs(3),
            position.wait_for(|seen| {
                seen.selection == 2 && seen.entry == Some(sentinel_entry) && seen.position_ms > 100
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(heard.entry, Some(sentinel_entry));
        let monitor = resources.capture.as_ref().unwrap();
        let audible_mark = monitor.mark();
        monitor.after(audible_mark, 0.1);
        let audible = monitor.recent(0.1);
        assert!(capture::amplitude(&audible, 880.) > 0.2);
        assert!(capture::amplitude(&audible, 440.) < 0.02);
        if check_owner {
            let inspection = player.inspect(&actor).await.unwrap();
            assert_eq!(inspection.epoch, attached.epoch);
            let current_state = inspection.state.unwrap();
            let paused_mark = monitor.mark();
            let receipt = player
                .submit(
                    &actor,
                    super::resources::command(
                        attached.lease,
                        3,
                        current_state.revision,
                        PlayerAction::Pause {},
                    ),
                )
                .await
                .unwrap();
            assert_eq!(receipt.sequence, 3);
            let paused = tokio::time::timeout(
                Duration::from_secs(3),
                state.wait_for(|snapshot| !snapshot.desired_playing && !snapshot.playing),
            )
            .await
            .unwrap()
            .unwrap()
            .clone();
            let inspection = player.inspect(&actor).await.unwrap();
            assert_eq!(inspection.epoch, attached.epoch);
            let paused_state = inspection.state.unwrap();
            assert!(!paused.desired_playing);
            assert!(!paused.playing);
            assert_eq!(paused_state.revision, current_state.revision);
            assert_eq!(paused_state.selection, selected.selection);
            assert_eq!(
                paused_state.current.as_ref().map(|entry| entry.id),
                Some(sentinel_entry)
            );
            assert!(!paused_state.desired_playing);
            assert!(!paused_state.playing);
            assert!(paused_state.retrying.is_none());
            assert!(paused_state.error.is_none());
            assert_eq!(paused_state.last_failure, selected.last_failure);
            monitor.after(paused_mark, 0.3);
            let paused = monitor.recent(0.1);
            assert!(capture::amplitude(&paused, 880.) < 0.02);
            assert!(capture::amplitude(&paused, 440.) < 0.02);
        }
    })
    .catch_unwind()
    .await;
    let cleanup = resources.finish(true).await;
    assert!(cleanup.silent);
    if behavior.is_ok() {
        assert!(
            matches!(&cleanup.output, Some(Ok(report)) if !report.callbacks.is_empty()),
            "sentinel output needs a complete witness: {:?}",
            cleanup.output
        );
    }
    match behavior {
        Ok(()) => (),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

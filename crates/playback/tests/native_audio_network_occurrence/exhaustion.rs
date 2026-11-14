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
async fn three_audio_unavailable_passes_advance_once_to_the_audible_sentinel() {
    let mut resources = Resources::new(fixture::Fixture::start().await);
    let (private, actor, recording, sentinel_id, services, order) = {
        let fixture = resources.fixture.as_ref().unwrap();
        (
            fixture.private_values(),
            fixture.occurrence.actor.clone(),
            fixture.occurrence.recording,
            fixture.occurrence.sentinel,
            fixture.occurrence.services.clone(),
            fixture.occurrence.order.clone(),
        )
    };
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    let gate = resources.hold_audio();
    let behavior = AssertUnwindSafe(async {
        resources.witness = Some(OutputWitness::begin(48_000 * 20, 4096).unwrap());
        resources.capture = Some(capture::Capture::open());
        resources.native = Some(Player::native(services));
        let (player, _) = resources.native.as_ref().unwrap();
        let attached = hang_guard("audio network occurrence claim", async {
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
                        recordings: vec![recording, sentinel_id],
                        start: 0,
                    },
                ),
            )
            .await
            .unwrap();
        let target = player
            .inspect(&actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .current
            .unwrap()
            .id;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        tokio::time::timeout_at(
            deadline,
            player.submit(
                &actor,
                super::resources::command(
                    attached.lease,
                    2,
                    queued.revision,
                    PlayerAction::Resume {},
                ),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        let mut attempt_audio = [0usize; 3];
        let initial_position = *position.borrow_and_update();
        let mut request_entered = false;
        loop {
            if !request_entered {
                tokio::time::timeout_at(deadline, gate.entered())
                    .await
                    .unwrap();
            }
            let snapshot = player.inspect(&actor).await.unwrap().state.unwrap();
            let attempt = snapshot
                .retrying
                .map_or(1, |retry| usize::from(retry.attempt));
            assert!((1..=3).contains(&attempt));
            assert_eq!(snapshot.selection, 1);
            assert_eq!(
                snapshot.current.as_ref().map(|entry| entry.id),
                Some(target)
            );
            assert_eq!(snapshot.revision, queued.revision);
            assert!(snapshot.last_failure.is_none());
            assert_eq!(*position.borrow_and_update(), initial_position);
            if attempt_audio.iter().sum::<usize>() == 0 {
                assert!(snapshot.retrying.is_none());
                assert!(snapshot.source_id.is_some());
            }
            attempt_audio[attempt - 1] += 1;
            assert!(snapshot.source_id.is_some());
            gate.release();
            let next_audio = tokio::time::timeout_at(deadline, async {
                tokio::select! {
                    biased;
                    result = async {
                        state.wait_for(|snapshot| snapshot.selection == 2)
                            .await.map(|snapshot| snapshot.clone())
                    } => {
                        result.unwrap();
                        false
                    },
                    _ = gate.entered() => true,
                }
            })
            .await
            .unwrap();
            if !next_audio {
                break;
            }
            request_entered = true;
        }
        let sentinel = tokio::time::timeout_at(
            deadline,
            state.wait_for(|snapshot| {
                snapshot.selection == 2
                    && snapshot.current_index == Some(1)
                    && snapshot.phase == PlayerPhase::Ready
                    && snapshot.playing
            }),
        )
        .await
        .unwrap()
        .unwrap()
        .clone();
        assert_eq!(
            sentinel.current.as_ref().map(|entry| entry.recording_id),
            Some(sentinel_id)
        );
        assert!(sentinel.retrying.is_none());
        assert_eq!(
            sentinel
                .last_failure
                .as_ref()
                .map(|failure| (failure.entry.id, failure.error)),
            Some((target, PlayerError::Playback))
        );
        let sentinel_entry = sentinel.current.as_ref().unwrap().id;
        let heard = *tokio::time::timeout_at(
            deadline,
            position.wait_for(|position| {
                position.selection == 2
                    && position.entry == Some(sentinel_entry)
                    && position.position_ms > 100
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(heard.position_ms > 100);
        assert!(attempt_audio.iter().all(|count| *count > 0));
        assert_eq!(
            resources.fixture.as_ref().unwrap().audio_requests(),
            attempt_audio.iter().sum::<usize>()
        );
        let order = order.lock().unwrap().clone();
        assert!(order.starts_with(&['A', 'B']));
        tracing::info!(
            event = "native_audio_unavailable_passes",
            pass_one_audio = attempt_audio[0],
            pass_two_audio = attempt_audio[1],
            pass_three_audio = attempt_audio[2],
            discovery_calls = order.len()
        );
        let monitor = resources.capture.as_ref().unwrap();
        monitor.after(monitor.mark(), 0.1);
        let samples = monitor.recent(0.1);
        assert!(capture::amplitude(&samples, 880.) > 0.2);
        assert!(capture::amplitude(&samples, 440.) < 0.02);
    })
    .catch_unwind()
    .with_subscriber(dispatch)
    .await;
    let cleanup = resources.finish(true).await;
    assert!(cleanup.silent);
    if behavior.is_ok() {
        assert!(
            matches!(&cleanup.output, Some(Ok(report)) if !report.callbacks.is_empty()),
            "sentinel output needs complete witness: {:?}",
            cleanup.output
        );
    }
    logs.assert_excludes(&private.iter().map(String::as_str).collect::<Vec<_>>());
    match behavior {
        Ok(()) => (),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

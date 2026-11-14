use super::super::fixture;
use super::resources::Resources;
use futures_util::FutureExt;
use hito_audio::test_support::capture;
use hito_audio::test_support::output_witness::OutputWitness;
use hito_core::test_support::hang_guard::hang_guard;
use hito_playback::{Player, PlayerAction, PlayerPhase, PlayerRetry};
use hito_testkit::logs::LogCapture;
use std::{panic::AssertUnwindSafe, time::Duration};
use tracing::instrument::WithSubscriber;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn audio_503_retries_the_same_occurrence_and_exposes_retry_two_until_heard_progress() {
    let f = fixture::Fixture::start().await;
    let private = f.private_values();
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    exercise_retry(f, false).with_subscriber(dispatch).await;
    logs.assert_excludes(&private.iter().map(String::as_str).collect::<Vec<_>>());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn preheader_disconnect_retries_the_same_occurrence() {
    let f = fixture::Fixture::start().await;
    f.disconnect_before_headers();
    let private = f.private_values();
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    exercise_retry(f, true).with_subscriber(dispatch).await;
    logs.assert_excludes(&private.iter().map(String::as_str).collect::<Vec<_>>());
}

async fn exercise_retry(fixture: fixture::Fixture, disconnect: bool) {
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
    let gate = (!disconnect).then(|| resources.hold_audio());
    let mut attempt_audio = [0usize; 3];
    let mut disconnected_requests = 0usize;
    let behavior = AssertUnwindSafe(async {
        resources.witness = Some(OutputWitness::begin(48_000 * 20, 4096).unwrap());
        resources.capture = Some(capture::Capture::open());
        resources.native = Some(Player::native(services));
        let (player, _) = resources.native.as_ref().unwrap();
        let attached = hang_guard("audio network retry claim", async {
            player
                .claim(
                    &actor,
                    player.inspect(&actor).await.unwrap().epoch,
                )
                .await
                .unwrap()
        })
        .await;
        let mut state = player
            .subscribe(&actor, attached.lease)
            .await
            .unwrap();
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
        let initial = player
            .inspect(&actor)
            .await
            .unwrap()
            .state
            .unwrap();
        let entry = initial.current.as_ref().unwrap().id;
        let initial_position = *position.borrow_and_update();
        player
            .submit(
                &actor,
                super::resources::command(attached.lease, 2, queued.revision, PlayerAction::Resume {}),
            )
            .await
            .unwrap();
        if disconnect {
            let grant = tokio::time::timeout(
                Duration::from_secs(3),
                state.wait_for(|snapshot| snapshot.selection == 1 && snapshot.source_id.is_some()),
            )
            .await
            .unwrap()
            .unwrap()
            .clone();
            assert!(grant.retrying.is_none());
            tokio::time::timeout(Duration::from_secs(3), resources.fixture.as_ref().unwrap().wait_audio_request(1))
                .await
                .unwrap();
            disconnected_requests = resources.fixture.as_ref().unwrap().audio_requests();
            assert!(disconnected_requests > 0);
            tokio::time::timeout(
                Duration::from_secs(3),
                state.wait_for(|snapshot| {
                    snapshot.retrying == Some(PlayerRetry { attempt: 2, of: 3 })
                }),
            )
            .await
            .unwrap()
            .unwrap();
            resources.fixture.as_ref().unwrap().clear_disconnect();
            let held = resources.fixture.as_ref().unwrap().hold_audio();
            resources.gates.lock().unwrap().push(held.clone());
            tokio::time::timeout(Duration::from_secs(3), held.entered())
                .await
                .unwrap();
            let retry_two = player
                .inspect(&actor)
                .await
                .unwrap()
                .state
                .unwrap();
            assert_eq!(retry_two.retrying, Some(PlayerRetry { attempt: 2, of: 3 }));
            assert_eq!(retry_two.current.as_ref().map(|item| item.id), Some(entry));
            assert_eq!(retry_two.revision, queued.revision);
            assert!(retry_two.last_failure.is_none());
            assert_eq!(*position.borrow_and_update(), initial_position);
            attempt_audio[1] += 1;
            resources.fixture.as_ref().unwrap().allow_audio();
            held.release();
            tokio::time::timeout(Duration::from_secs(3), held.entered())
                .await
                .unwrap();
            let retry_three = player
                .inspect(&actor)
                .await
                .unwrap()
                .state
                .unwrap();
            assert_eq!(
                retry_three.retrying,
                Some(PlayerRetry { attempt: 3, of: 3 })
            );
            assert!(retry_three.source_id.is_some());
            assert!(retry_three.last_failure.is_none());
            assert_eq!(*position.borrow_and_update(), initial_position);
            attempt_audio[2] += 1;
            held.release();
        } else {
            let held = gate.as_ref().unwrap();
            loop {
                let entered = tokio::time::timeout(Duration::from_secs(3), async {
                    tokio::select! {
                        biased;
                        result = async {
                            state.wait_for(|snapshot| snapshot.selection != 1 || snapshot.last_failure.is_some())
                                .await.map(|snapshot| snapshot.clone())
                        } => {
                            result.unwrap();
                            false
                        },
                        _ = held.entered() => true,
                    }
                }).await.unwrap();
                if !entered {
                    assert_eq!(resources.fixture.as_ref().unwrap().audio_requests(), 1);
                    assert_eq!(resources.fixture.as_ref().unwrap().handler_calls(), 1);
                    assert_eq!(resources.fixture.as_ref().unwrap().unavailable_responses(), 1);
                    let failed = player.inspect(&actor).await.unwrap().state.unwrap();
                    eprintln!("native_audio_retry_missing audio_requests={} handler_calls={} selection={} phase={:?} source_selected={} desired_playing={} last_failure={:?}", resources.fixture.as_ref().unwrap().audio_requests(), resources.fixture.as_ref().unwrap().handler_calls(), failed.selection, failed.phase, failed.source_id.is_some(), failed.desired_playing, failed.last_failure.as_ref().map(|failure| failure.error));
                    tracing::info!(event = "native_audio_retry_missing", audio_requests = resources.fixture.as_ref().unwrap().audio_requests(), audio_handler_calls = resources.fixture.as_ref().unwrap().handler_calls(), selection = failed.selection, last_failure = ?failed.last_failure.as_ref().map(|failure| failure.error), "Granted audio response ended the target occurrence before retry two");
                    panic!("audio 503 ended target acquisition without retry 2 of 3");
                }
                let snapshot = player
                    .inspect(&actor)
                    .await
                    .unwrap()
                    .state
                    .unwrap();
                let attempt = snapshot
                    .retrying
                    .map_or(1, |retry| usize::from(retry.attempt));
                assert!(attempt == 1 || attempt == 2);
                assert_eq!(snapshot.selection, 1);
                assert_eq!(snapshot.revision, queued.revision);
                assert_eq!(snapshot.current.as_ref().map(|item| item.id), Some(entry));
                assert!(snapshot.last_failure.is_none());
                assert_eq!(*position.borrow_and_update(), initial_position);
                attempt_audio[attempt - 1] += 1;
                if attempt == 1 {
                    assert!(snapshot.source_id.is_some());
                    held.release();
                    continue;
                }
                assert_eq!(attempt, 2);
                assert!(snapshot.source_id.is_some());
                resources.fixture.as_ref().unwrap().allow_audio();
                held.release();
                break;
            }
            tokio::time::timeout(
                Duration::from_secs(3),
                state.wait_for(|snapshot| {
                    snapshot.retrying == Some(PlayerRetry { attempt: 3, of: 3 })
                }),
            )
            .await
            .unwrap()
            .unwrap();
            tokio::time::timeout(Duration::from_secs(3), held.entered())
                .await
                .unwrap();
            let retry_three = player
                .inspect(&actor)
                .await
                .unwrap()
                .state
                .unwrap();
            assert_eq!(
                retry_three.retrying,
                Some(PlayerRetry { attempt: 3, of: 3 })
            );
            assert!(retry_three.source_id.is_some());
            assert!(retry_three.last_failure.is_none());
            assert_eq!(*position.borrow_and_update(), initial_position);
            attempt_audio[2] += 1;
            held.release();
        }
        let observed = *tokio::time::timeout(
            Duration::from_secs(5),
            position.wait_for(|position| {
                position.selection == 1
                    && position.entry == Some(entry)
                    && position.position_ms > 100
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(observed.position_ms > 100);
        let final_state = player
            .inspect(&actor)
            .await
            .unwrap()
            .state
            .unwrap();
        assert_eq!(final_state.selection, 1);
        assert_eq!(
            final_state.current.as_ref().map(|current| current.id),
            Some(entry)
        );
        assert_eq!(final_state.revision, queued.revision);
        assert_eq!(final_state.phase, PlayerPhase::Ready);
        assert!(final_state.playing);
        assert!(final_state.retrying.is_none());
        assert!(final_state.error.is_none());
        assert!(final_state.last_failure.is_none());
        let monitor = resources.capture.as_ref().unwrap();
        monitor.after(monitor.mark(), 0.1);
        let samples = monitor.recent(0.1);
        assert!(capture::amplitude(&samples, 440.) > 0.2);
        assert!(capture::amplitude(&samples, 880.) < 0.02);
        assert_eq!(attempt_audio[1..], [1, 1]);
        assert!(if disconnect {
            disconnected_requests > 0
        } else {
            attempt_audio[0] > 0
        });
        assert_eq!(
            resources.fixture.as_ref().unwrap().audio_requests(),
            attempt_audio.iter().sum::<usize>() + disconnected_requests
        );
        assert_eq!(resources.fixture.as_ref().unwrap().handler_calls(), attempt_audio.iter().sum::<usize>());
        let order = order.lock().unwrap().clone();
        assert!(order.starts_with(&['A', 'B']));
        tracing::info!(
            event = "native_audio_retry_passes",
            preheader_disconnect = disconnect,
            attempt_one_audio = attempt_audio[0],
            attempt_two_audio = attempt_audio[1],
            attempt_three_audio = attempt_audio[2],
            disconnected_requests,
            discovery_calls = order.len()
        );
    })
    .catch_unwind()
    .await;
    let cleanup = resources.finish(false).await;
    assert!(cleanup.silent && cleanup.target_audio < 0.02);
    if behavior.is_ok() {
        assert!(matches!(&cleanup.output, Some(Ok(report)) if !report.callbacks.is_empty()));
    }
    match behavior {
        Ok(()) => (),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

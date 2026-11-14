use super::super::fixture;
use super::resources::Resources;
use futures_util::FutureExt;
use hito_audio::test_support::capture;
use hito_audio::test_support::output_witness::OutputWitness;
use hito_core::test_support::hang_guard::hang_guard;
use hito_playback::{Player, PlayerAction, PlayerRetry};
use hito_testkit::logs::LogCapture;
use std::{panic::AssertUnwindSafe, time::Duration};
use tracing::instrument::WithSubscriber;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn pause_cancels_retry_two_audio_after_receipt() {
    run_cancel(Cancel::Pause).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn next_cancels_retry_two_audio_after_receipt() {
    run_cancel(Cancel::Next).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn previous_cancels_retry_two_audio_after_receipt() {
    run_cancel(Cancel::Previous).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn select_cancels_retry_two_audio_after_receipt() {
    run_cancel(Cancel::Select).await;
}

async fn run_cancel(cancel: Cancel) {
    let logs = LogCapture::default();
    let dispatch = logs.dispatch(
        "off,hito_core=info,hito_playback=info,hito_playback::player::effects::sources=debug",
    );
    exercise_cancel(cancel, &logs)
        .with_subscriber(dispatch)
        .await;
}

#[derive(Clone, Copy)]
enum Cancel {
    Pause,
    Next,
    Previous,
    Select,
}

async fn exercise_cancel(cancel: Cancel, logs: &LogCapture) {
    let mut resources = Resources::new(fixture::Fixture::start().await);
    let (private, actor, recording, sentinel, services) = {
        let fixture = resources.fixture.as_ref().unwrap();
        (
            fixture.private_values(),
            fixture.occurrence.actor.clone(),
            fixture.occurrence.recording,
            fixture.occurrence.sentinel,
            fixture.occurrence.services.clone(),
        )
    };
    let behavior = AssertUnwindSafe(async {
        resources.witness = Some(OutputWitness::begin(48_000 * 20, 4096).unwrap());
        resources.capture = Some(capture::Capture::open());
        resources.native = Some(Player::native(services));
        let (player, _) = resources.native.as_ref().unwrap();
        let attached = hang_guard("audio retry cancellation claim", async {
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
        let previous = matches!(cancel, Cancel::Previous);
        let recordings = if previous {
            vec![sentinel]
        } else {
            vec![recording, sentinel]
        };
        let queued = player
            .submit(
                &actor,
                super::resources::command(
                    attached.lease,
                    1,
                    0,
                    PlayerAction::Replace {
                        recordings,
                        start: 0,
                    },
                ),
            )
            .await
            .unwrap();
        let mut revision = queued.revision;
        let mut sequence = 2;
        let mut sentinel_entry = player
            .queue(&actor, attached.lease, revision, 0, 10)
            .await
            .unwrap()
            .entries[if previous { 0 } else { 1 }]
        .id;
        player
            .submit(
                &actor,
                super::resources::command(
                    attached.lease,
                    sequence,
                    revision,
                    PlayerAction::Resume {},
                ),
            )
            .await
            .unwrap();
        sequence += 1;
        let target_selection = if previous {
            let heard = *tokio::time::timeout(
                Duration::from_secs(3),
                position.wait_for(|position| position.selection == 1 && position.position_ms > 100),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(heard.entry, Some(sentinel_entry));
            let appended = player
                .submit(
                    &actor,
                    super::resources::command(
                        attached.lease,
                        sequence,
                        revision,
                        PlayerAction::Append {
                            recordings: vec![recording],
                        },
                    ),
                )
                .await
                .unwrap();
            sequence += 1;
            revision = appended.revision;
            sentinel_entry = player
                .queue(&actor, attached.lease, revision, 0, 10)
                .await
                .unwrap()
                .entries[0]
                .id;
            let next = player
                .submit(
                    &actor,
                    super::resources::command(
                        attached.lease,
                        sequence,
                        revision,
                        PlayerAction::Next {},
                    ),
                )
                .await
                .unwrap();
            sequence += 1;
            revision = next.revision;
            2
        } else {
            1
        };
        let target = player
            .inspect(&actor)
            .await
            .unwrap()
            .state
            .unwrap()
            .current
            .unwrap()
            .id;
        let target_position = *position.borrow_and_update();
        tokio::time::timeout(
            Duration::from_secs(3),
            state.wait_for(|snapshot| snapshot.retrying == Some(PlayerRetry { attempt: 2, of: 3 })),
        )
        .await
        .unwrap()
        .unwrap();
        let gate = resources.fixture.as_ref().unwrap().hold_audio();
        resources.gates.lock().unwrap().push(gate.clone());
        tokio::time::timeout(Duration::from_secs(3), gate.entered())
            .await
            .unwrap();
        let held = player.inspect(&actor).await.unwrap().state.unwrap();
        assert_eq!(held.retrying, Some(PlayerRetry { attempt: 2, of: 3 }));
        assert_eq!(held.selection, target_selection);
        assert_eq!(held.revision, revision);
        assert_eq!(held.current.as_ref().map(|entry| entry.id), Some(target));
        assert!(held.last_failure.is_none());
        assert!(!held.playing);
        assert!(held.source_id.is_some());
        assert_eq!(*position.borrow_and_update(), target_position);
        let monitor = resources.capture.as_ref().unwrap();
        let release_mark = monitor.mark();
        let action = match cancel {
            Cancel::Pause => PlayerAction::Pause {},
            Cancel::Next => PlayerAction::Next {},
            Cancel::Previous => PlayerAction::Previous {},
            Cancel::Select => PlayerAction::Select {
                entry: sentinel_entry,
            },
        };
        let receipt = tokio::time::timeout(
            Duration::from_secs(3),
            player.submit(
                &actor,
                super::resources::command(attached.lease, sequence, revision, action),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        let after = player.inspect(&actor).await.unwrap().state.unwrap();
        assert!(after.retrying.is_none());
        assert!(after.last_failure.is_none());
        assert!(after.error.is_none());
        if matches!(cancel, Cancel::Pause) {
            assert!(!after.desired_playing);
            assert_eq!(after.selection, target_selection);
            assert_eq!(after.current.as_ref().map(|entry| entry.id), Some(target));
            assert_eq!(after.source_id, None);
            assert!(!after.playing);
            assert_eq!(*position.borrow_and_update(), target_position);
        } else {
            assert_eq!(after.selection, target_selection + 1);
            assert_eq!(
                after.current.as_ref().map(|entry| entry.id),
                Some(sentinel_entry)
            );
        }
        gate.release();
        assert!(gate.retired_within(Duration::from_secs(3)).await);
        monitor.after(release_mark, 0.3);
        let settled = player.inspect(&actor).await.unwrap().state.unwrap();
        assert!(settled.retrying.is_none());
        assert!(settled.last_failure.is_none());
        assert!(settled.error.is_none());
        assert_eq!(settled.revision, receipt.revision);
        assert_eq!(
            settled.current.as_ref().map(|entry| entry.id),
            after.current.as_ref().map(|entry| entry.id)
        );
        if matches!(cancel, Cancel::Pause) {
            assert!(!settled.desired_playing);
            assert!(!settled.playing);
            assert!(capture::amplitude(&monitor.recent(0.1), 440.) < 0.02);
        } else {
            let heard = *tokio::time::timeout(
                Duration::from_secs(3),
                position.wait_for(|seen| {
                    seen.selection == target_selection + 1 && seen.position_ms > 100
                }),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(heard.entry, Some(sentinel_entry));
            monitor.after(monitor.mark(), 0.1);
            let samples = monitor.recent(0.1);
            assert!(capture::amplitude(&samples, 880.) > 0.2);
            assert!(capture::amplitude(&samples, 440.) < 0.02);
            let final_state = player.inspect(&actor).await.unwrap().state.unwrap();
            assert!(final_state.retrying.is_none());
            assert!(final_state.error.is_none());
            assert!(final_state.last_failure.is_none());
            assert_eq!(
                final_state.current.as_ref().map(|entry| entry.id),
                Some(sentinel_entry)
            );
        }
    })
    .catch_unwind()
    .await;
    let cleanup = resources.finish(true).await;
    assert!(cleanup.silent && cleanup.target_audio < 0.02);
    if behavior.is_ok() {
        if matches!(cancel, Cancel::Pause) {
            assert!(matches!(
                &cleanup.output,
                Some(Ok(_))
                    | Some(Err(
                        hito_audio::test_support::output_witness::WitnessError::Incomplete
                    ))
            ));
        } else {
            assert!(matches!(&cleanup.output, Some(Ok(report)) if !report.callbacks.is_empty()));
        }
    }
    logs.assert_excludes(&private.iter().map(String::as_str).collect::<Vec<_>>());
    match behavior {
        Ok(()) => (),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

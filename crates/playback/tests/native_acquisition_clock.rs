#![cfg(feature = "device")]

#[path = "fixtures/native_acquisition_clock.rs"]
mod fixture;

use futures_util::FutureExt;
use hito_audio::test_support::{capture, output_witness::OutputWitness};
use hito_core::test_support::hang_guard::hang_guard;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError, PlayerPhase};
use hito_testkit::logs::LogCapture;
use serde_json::Value;
use std::{
    panic::AssertUnwindSafe,
    time::{Duration, Instant},
};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn cached_provider_acquisition_uses_one_fifteen_second_clock() {
    let f = fixture::Fixture::start().await;
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    exercise(f, &logs).with_subscriber(dispatch).await;
}

async fn exercise(f: fixture::Fixture, logs: &LogCapture) {
    let mut witness = None;
    let mut monitor = None;
    let mut native = None;
    let mut first_gate = None;
    let mut second_tail = None;
    let mut state = None;
    let mut position = None;
    let mut resume_started = None;
    let mut deadline_observed = None;
    let mut target_entry = None;
    let mut sentinel_entry = None;
    let mut target_progressed = false;
    let mut successful_queue_state = None;
    let mut tail_dropped_before_release = false;
    let mut tail_entered = false;

    let behavior = AssertUnwindSafe(async {
        witness = Some(OutputWitness::begin(48_000 * 20, 4096).unwrap());
        monitor = Some(capture::Capture::open());
        let target_wave = fixture::wave(440.0);
        f.first
            .audio(
                axum::http::StatusCode::OK,
                hito_core::test_support::provider_audio::headers(),
                target_wave.clone(),
            )
            .await;
        f.second
            .audio(
                axum::http::StatusCode::OK,
                hito_core::test_support::provider_audio::headers(),
                target_wave,
            )
            .await;
        first_gate = Some(f.first.hold("v1/audio").await);
        second_tail = Some(f.hold_second_tail(1));

        let preferences = f
            .services
            .catalog
            .source_preferences(&f.actor)
            .await
            .unwrap();
        if preferences.entries
            != vec![
                hito_core::SourcePreferenceEntry::LocalFiles,
                hito_core::SourcePreferenceEntry::SavedCopies,
                hito_core::SourcePreferenceEntry::Provider {
                    installation_id: f.first_installation_id.clone(),
                },
                hito_core::SourcePreferenceEntry::Provider {
                    installation_id: f.second_installation_id.clone(),
                },
            ]
        {
            return Err("public source preferences did not retain the fixture's provider order");
        }
        let mut candidates = f
            .services
            .playback
            .playback_candidates(&f.actor, &f.recording.to_string())
            .await
            .unwrap();
        let first = candidates.next().await.unwrap().unwrap();
        let second = candidates.next().await.unwrap().unwrap();
        if first.source_id() != f.first_source_id || second.source_id() != f.second_source_id {
            return Err(
                "public candidates did not select the exact first and second attached source IDs",
            );
        }

        native = Some(Player::native(f.services.clone()));
        let (player, _) = native.as_ref().unwrap();
        let attached = hang_guard("native acquisition clock claim", async {
            let epoch = player.inspect(&f.actor).await.unwrap().epoch;
            player.claim(&f.actor, epoch).await.unwrap()
        })
        .await;
        state = Some(player.subscribe(&f.actor, attached.lease).await.unwrap());
        position = Some(
            player
                .subscribe_position(&f.actor, attached.lease)
                .await
                .unwrap(),
        );
        let queued = player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence: 1,
                    revision: 0,
                    action: PlayerAction::Replace {
                        recordings: vec![f.recording, f.sentinel],
                        start: 0,
                    },
                },
            )
            .await
            .unwrap();
        let initial = player.inspect(&f.actor).await.unwrap().state.unwrap();
        target_entry = initial.current.as_ref().map(|entry| entry.id);
        let resume = Instant::now();
        resume_started = Some(resume);
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence: 2,
                    revision: queued.revision,
                    action: PlayerAction::Resume {},
                },
            )
            .await
            .unwrap();

        let first_gate = first_gate.as_ref().unwrap();
        tokio::time::timeout(Duration::from_secs(3), first_gate.entered())
            .await
            .map_err(|_| "first provider audio request did not reach the held-header gate")?;
        let tail = second_tail.as_ref().unwrap();
        tokio::time::timeout(Duration::from_secs(13), tail.entered())
            .await
            .map_err(
                |_| "second provider body did not enter within its phase and observation allowance",
            )?;
        tail_entered = true;

        let deadline = resume + Duration::from_secs(18);
        let position = position.as_mut().unwrap();
        let progressed =
            match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
                loop {
                    position
                        .changed()
                        .await
                        .map_err(|_| "the position observer closed before sentinel progress")?;
                    let observed = *position.borrow_and_update();
                    if observed.selection == 1 && observed.position_ms > 0 {
                        target_progressed = true;
                    }
                    if observed.selection == 2 && observed.position_ms > 100 {
                        break Ok::<_, &'static str>(observed);
                    }
                }
            })
            .await
            {
                Err(_) => {
                    deadline_observed = Some(Instant::now());
                    return Err(
                        "sentinel made no progressing output by Resume plus eighteen seconds",
                    );
                }
                Ok(Err(error)) => return Err(error),
                Ok(Ok(progress)) => progress,
            };
        if target_progressed {
            return Err("the target published a progressing position");
        }
        sentinel_entry = progressed.entry;
        let observed = player.inspect(&f.actor).await.unwrap().state.unwrap();
        if observed.selection != 2
            || observed.current_index != Some(1)
            || observed.current.as_ref().is_none_or(|entry| {
                Some(entry.id) != sentinel_entry || entry.recording_id != f.sentinel
            })
        {
            return Err("the sentinel occurrence was not current after acquisition");
        }
        if observed.phase != PlayerPhase::Ready
            || !observed.playing
            || observed.last_failure.as_ref().is_none_or(|failure| {
                Some(failure.entry.id) != target_entry || failure.error != PlayerError::Playback
            })
        {
            return Err(
                "the player did not retain exactly the target failure while the sentinel played",
            );
        }
        let current = observed.current.as_ref().unwrap();
        successful_queue_state = Some((
            observed.selection,
            observed.current_index,
            Some(current.id),
            Some(current.recording_id),
        ));

        let tail = second_tail.as_ref().unwrap();
        tokio::time::timeout(Duration::from_secs(3), tail.dropped())
            .await
            .map_err(
                |_| "the expired target's second body remained active after sentinel progress",
            )?;
        tail_dropped_before_release = true;

        let monitor = monitor.as_ref().unwrap();
        monitor.after(monitor.mark(), 0.1);
        let samples = monitor.recent(0.1);
        if capture::amplitude(&samples, 880.0) <= 0.2 || capture::amplitude(&samples, 440.0) >= 0.02
        {
            return Err("captured native output was not the 880Hz sentinel alone");
        }
        Ok(())
    })
    .catch_unwind()
    .await;

    let success = matches!(&behavior, Ok(Ok(())));
    let owner_created = native.is_some();
    let mut gate_released = false;
    let mut tail_dropped = tail_dropped_before_release || !tail_entered;
    let mut owner_close_error = None;
    let mut queue_stable_after_release = !success;
    let mut requests_stable_after_release = !success;
    if !success {
        if let Some(gate) = first_gate.as_ref() {
            gate.release();
            gate_released = true;
        }
        if let Some(tail) = second_tail.as_ref() {
            tail.release();
        }
    }
    if success {
        if tail_dropped_before_release {
            if let Some(gate) = first_gate.as_ref() {
                gate.release();
                gate_released = true;
            }
            if let Some(tail) = second_tail.as_ref() {
                tail.release();
            }
        }
        if let (Some((player, _)), Some(expected)) = (native.as_ref(), successful_queue_state)
            && let Ok(Ok(inspected)) =
                tokio::time::timeout(Duration::from_secs(2), player.inspect(&f.actor)).await
        {
            let actual = inspected.state.as_ref().and_then(|state| {
                state.current.as_ref().map(|entry| {
                    (
                        state.selection,
                        state.current_index,
                        Some(entry.id),
                        Some(entry.recording_id),
                    )
                })
            });
            queue_stable_after_release = actual == Some(expected);
        }
        requests_stable_after_release = f.first.requests("v1/audio").await.len() == 1
            && f.second.requests("v1/audio").await.len() == 1;
    }

    let (mut player, mut owner_close) = match native.take() {
        Some((player, owner)) => (Some(player), Some(Box::pin(owner.close()))),
        None => (None, None),
    };
    let owner_joined = if let Some(close) = owner_close.as_mut() {
        match tokio::time::timeout(
            Duration::from_secs(3),
            AssertUnwindSafe(close.as_mut()).catch_unwind(),
        )
        .await
        {
            Ok(Ok(result)) => {
                owner_close_error = result.as_ref().err().map(|error| format!("{error:?}"));
                result.is_ok()
            }
            Ok(Err(_)) => false,
            Err(_) => AssertUnwindSafe(close.as_mut())
                .catch_unwind()
                .await
                .is_ok_and(|result| result.is_ok()),
        }
    } else {
        true
    };
    drop(owner_close.take());
    drop(player.take());
    if !success
        && tail_entered
        && !tail_dropped
        && let Some(tail) = second_tail.as_ref()
    {
        tail_dropped = tokio::time::timeout(Duration::from_secs(3), tail.dropped())
            .await
            .is_ok();
    }
    drop(state.take());
    drop(position.take());

    let output = witness.as_ref().map(OutputWitness::finish);
    let output_ok = matches!(&output, Some(Ok(_)));
    let output_callbacks = output
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .map(|report| report.callbacks.len())
        .unwrap_or(0);
    let active_writers = matches!(
        &output,
        Some(Err(
            hito_audio::test_support::output_witness::WitnessError::ActiveWriters
        ))
    );
    let overflow = matches!(
        &output,
        Some(Err(
            hito_audio::test_support::output_witness::WitnessError::Overflow
        ))
    );
    let monitor_silent_after_close = monitor.as_ref().is_none_or(|monitor| {
        monitor.after(monitor.mark(), 0.3);
        capture::energy(&monitor.recent(0.1)) < 0.000001
    });
    drop(witness.take());
    drop(monitor.take());

    let preserved = AssertUnwindSafe(f.preserved(success))
        .catch_unwind()
        .await
        .is_ok_and(|preserved| preserved);
    let first_audio_requests =
        tokio::time::timeout(Duration::from_secs(2), f.first.requests("v1/audio"))
            .await
            .map(|requests| requests.len())
            .unwrap_or_default();
    let second_audio_requests =
        tokio::time::timeout(Duration::from_secs(2), f.second.requests("v1/audio"))
            .await
            .map(|requests| requests.len())
            .unwrap_or_default();
    let cleanup = f.close().await;
    let behavior_panic = behavior
        .as_ref()
        .err()
        .and_then(|payload| payload.downcast_ref::<&str>().copied())
        .or_else(|| {
            behavior
                .as_ref()
                .err()
                .and_then(|payload| payload.downcast_ref::<String>().map(String::as_str))
        });
    let behavior_error = behavior
        .as_ref()
        .ok()
        .and_then(|result| result.as_ref().err().copied());
    let deadline_elapsed_ms = deadline_observed
        .zip(resume_started)
        .map(|(deadline, resume)| deadline.duration_since(resume).as_millis());
    eprintln!(
        "native acquisition fixture verdict: behavior_error={behavior_error:?} behavior_panic={behavior_panic:?} owner_created={owner_created} owner_joined={owner_joined} tail_entered={tail_entered} tail_dropped={tail_dropped} gate_released={gate_released} target_progressed={target_progressed} deadline_elapsed_ms={deadline_elapsed_ms:?} first_audio_requests={first_audio_requests} second_audio_requests={second_audio_requests} output_callbacks={output_callbacks} output_ok={output_ok} active_writers={active_writers} overflow={overflow} preserved={preserved} providers_closed=({}, {}) database_retired={} database_deadline_exceeded={}",
        cleanup.first_provider_closed,
        cleanup.second_provider_closed,
        cleanup.database_retired,
        cleanup.database_deadline_exceeded
    );
    tracing::info!(
        event = "native_acquisition_clock_fixture_closed",
        owner_joined,
        tail_dropped,
        gate_released,
        active_writers,
        overflow,
        output_callbacks,
        monitor_silent_after_close,
        preserved,
        first_audio_requests,
        second_audio_requests,
        first_provider_closed = cleanup.first_provider_closed,
        second_provider_closed = cleanup.second_provider_closed,
        database_retired = cleanup.database_retired,
        database_deadline_exceeded = cleanup.database_deadline_exceeded,
        deadline_elapsed_ms,
        owner_close_error,
        behavior_panic,
        behavior_error,
        "Owned native acquisition fixture closed"
    );
    let logs_clean = logs.events().iter().all(|event: &Value| {
        let rendered = event.to_string();
        !rendered.contains("owned acquisition clock password")
            && !rendered.contains("Fixture track")
            && !rendered.contains("private-audio-token")
    });

    assert!(
        owner_joined,
        "native owner did not join; acquisition behavior error: {:?}; panic: {:?}; owner close error: {:?}",
        behavior_error, behavior_panic, owner_close_error
    );
    assert!(tail_dropped, "the entered held body was not dropped");
    assert!(
        gate_released,
        "the first provider header gate did not release"
    );
    assert!(
        !active_writers && !overflow,
        "output witness rejected active writers or overflow: {output:?}"
    );
    assert!(
        !target_progressed,
        "the target published a progressing position"
    );
    assert!(
        monitor_silent_after_close,
        "native output continued after player shutdown"
    );
    assert!(
        preserved,
        "owned files or private account state changed unexpectedly"
    );
    assert_eq!(
        first_audio_requests, 1,
        "first cached source must be attempted once; behavior error: {behavior_error:?}"
    );
    assert_eq!(
        second_audio_requests, 1,
        "second cached source must be attempted once; behavior error: {behavior_error:?}"
    );
    assert!(logs_clean, "captured logs included private provider data");
    assert!(
        cleanup.first_provider_closed && cleanup.second_provider_closed,
        "both content providers must be stopped"
    );
    assert!(
        cleanup.database_retired && !cleanup.database_deadline_exceeded,
        "Core's owned SQLite resources did not retire cleanly"
    );
    if success {
        let events = logs.events();
        let deadlines = events
            .iter()
            .filter(|event| event["fields"]["event"] == "player_acquisition_deadline")
            .collect::<Vec<_>>();
        assert_eq!(
            deadlines.len(),
            1,
            "the desired acquisition run must hit one deadline"
        );
        let deadline_id = deadlines[0]["fields"]["request_id"]
            .as_str()
            .expect("deadline event must carry the existing demand request ID");
        assert!(
            Uuid::parse_str(deadline_id).is_ok_and(|id| !id.is_nil()),
            "the target acquisition must use a non-nil demand request ID"
        );
        let matching_starts = events
            .iter()
            .filter(|event| {
                event["fields"]["event"] == "player_acquisition_started"
                    && event["fields"]["request_id"] == deadline_id
            })
            .collect::<Vec<_>>();
        assert_eq!(
            matching_starts.len(),
            1,
            "the target deadline must correlate to exactly one start; sentinel runs may start independently"
        );
        let elapsed = deadlines[0]["fields"]["elapsed_ms"]
            .as_u64()
            .expect("deadline event must report elapsed time");
        assert!(
            (14_900..=15_250).contains(&elapsed),
            "production deadline elapsed {elapsed}ms; expected 15s with 250ms scheduler tolerance"
        );
        let attempts = deadlines[0]["fields"]["attempts"]
            .as_u64()
            .expect("deadline event must report logical attempt count");
        assert!(
            (1..=3).contains(&attempts),
            "acquisition run reported an invalid logical attempt count: {attempts}"
        );
        assert!(
            queue_stable_after_release,
            "releasing old gates changed the current occurrence"
        );
        assert!(
            requests_stable_after_release,
            "releasing old gates caused another old audio request"
        );
        assert!(
            output_ok && output_callbacks > 0,
            "sentinel success requires a complete output witness with callbacks; got {output:?}"
        );
    }
    let behavior_result = match behavior {
        Ok(result) => result,
        Err(_) => panic!("acquisition behavior panicked before producing a verdict"),
    };
    assert!(
        behavior_result.is_ok(),
        "sentinel did not progress by Resume+18s: {:?}; deadline observation elapsed={:?}ms",
        behavior_result.err(),
        deadline_elapsed_ms
    );
}

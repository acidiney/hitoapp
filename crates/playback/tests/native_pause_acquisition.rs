#![cfg(feature = "device")]

#[path = "fixtures/occurrence_retry_transient.rs"]
mod fixture;

use futures_util::FutureExt;
use hito_audio::test_support::{
    capture,
    output_witness::{OutputWitness, WitnessError},
};
use hito_core::{ResolveSource, SourceResolutionOutcome, test_support::hang_guard::hang_guard};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use hito_testkit::logs::{LogCapture, anchor_callsite_interest};
use serde_json::Value;
use std::{
    io::Write,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    PauseOnly,
    PauseThenResume,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn pause_retires_held_native_acquisition_without_advancing_or_playing() {
    run(Scenario::PauseOnly).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_paused_native_acquisition_resumes_from_its_seek_baseline() {
    run(Scenario::PauseThenResume).await;
}

async fn run(scenario: Scenario) {
    let f = match scenario {
        Scenario::PauseOnly => fixture::start().await,
        Scenario::PauseThenResume => fixture::start_for_resume().await,
    };
    let logs = LogCapture::default();
    let cancelled = CancelledDiscovery::default();
    anchor_callsite_interest();
    let log_writer = logs.clone();
    let event_writer = cancelled.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .with_env_filter("off,hito_core=debug,hito_playback=debug")
            .with_ansi(false)
            .without_time()
            .json()
            .with_writer((move || log_writer.clone()).and(move || event_writer.clone()))
            .finish(),
    );
    exercise(f, &logs, &cancelled, scenario)
        .with_subscriber(dispatch)
        .await;
}

#[derive(Clone, Default)]
struct CancelledDiscovery {
    request_id: Arc<Mutex<Option<Uuid>>>,
    ready: Arc<Notify>,
}

impl CancelledDiscovery {
    fn arm(&self, request_id: Uuid) {
        *self.request_id.lock().unwrap() = Some(request_id);
    }

    async fn wait(&self) {
        self.ready.notified().await;
    }
}

impl Write for CancelledDiscovery {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Ok(event) = serde_json::from_slice::<Value>(bytes) {
            let fields = &event["fields"];
            let request_id = self.request_id.lock().unwrap();
            if fields["event"] == "player_source_discovery_attempt_finished"
                && fields["request_id"]
                    .as_str()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    == *request_id
                && fields["reason"] == "cancelled"
                && fields["outcome"] == "cancelled"
            {
                self.ready.notify_one();
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn exercise(
    f: fixture::Fixture,
    logs: &LogCapture,
    cancelled: &CancelledDiscovery,
    scenario: Scenario,
) {
    let private_values = f.private_values();
    let mut witness = None;
    let mut monitor = None;
    let mut native = None;
    let mut first_gate = None;
    let mut second_gate = None;
    let mut retained = None;
    let behavior = AssertUnwindSafe(async {
        witness = Some(OutputWitness::begin(48_000 * 20, 4096).unwrap());
        monitor = Some(capture::Capture::open());
        native = Some(Player::native(f.services.clone()));
        let (player, _) = native.as_ref().unwrap();
        let first_gate = first_gate.insert(f.second.hold_next("v1/search"));
        let attached = hang_guard("owned pause acquisition claim", async {
            let epoch = player.inspect(&f.actor).await.unwrap().epoch;
            player.claim(&f.actor, epoch).await.unwrap()
        }).await;
        let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
        let queued = player.submit(&f.actor, PlayerCommand {
            id: Uuid::new_v4(), lease: attached.lease, sequence: 1, revision: 0,
            action: PlayerAction::Replace { recordings: vec![f.recording, f.sentinel], start: 0 },
        }).await.unwrap();
        let initial = player.inspect(&f.actor).await.unwrap().state.unwrap();
        let target_entry = initial.current.as_ref().unwrap().id;
        let target_selection = initial.selection;
        let original_queue = player.queue(&f.actor, attached.lease, queued.revision, 0, 10)
            .await.unwrap();
        let original_entries: Value = serde_json::to_value(&original_queue.entries).unwrap();
        let resumed = player.submit(&f.actor, PlayerCommand {
            id: Uuid::new_v4(), lease: attached.lease, sequence: 2, revision: queued.revision,
            action: PlayerAction::Resume {},
        }).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), first_gate.entered()).await
            .map_err(|_| "first B search did not reach its gate")?;
        let second_gate = second_gate.insert(f.second.hold_next("v1/search"));
        first_gate.release();
        tokio::time::timeout(Duration::from_secs(3), second_gate.entered()).await
            .map_err(|_| "second B search did not reach its gate")?;
        let attempts = logs.events().iter()
            .filter(|event| event["fields"]["event"] == "player_source_discovery_attempt_started")
            .filter_map(|event| Some((
                event["fields"]["attempt"].as_u64()?,
                event["fields"]["selection"].as_u64()?,
                Uuid::parse_str(event["fields"]["request_id"].as_str()?).ok()?,
            )))
            .collect::<Vec<_>>();
        let first_request = attempts.iter().find(|(attempt, selection, _)|
            *attempt == 1 && *selection == target_selection)
            .map(|(_, _, request)| *request)
            .ok_or("first target discovery attempt did not publish its identity")?;
        let request_id = attempts.iter().find(|(attempt, selection, _)|
            *attempt == 2 && *selection == target_selection)
            .map(|(_, _, request)| *request)
            .ok_or("held second target discovery attempt did not publish its identity")?;
        if request_id.is_nil() || request_id == first_request {
            return Err("held retry did not own a distinct non-nil request identity");
        }
        let seek_id = if scenario == Scenario::PauseThenResume {
            let id = Uuid::new_v4();
            let seek = player.submit(&f.actor, PlayerCommand {
                id, lease: attached.lease, sequence: 3, revision: resumed.revision,
                action: PlayerAction::Seek { position_ms: 1000 },
            }).await.unwrap();
            Some((id, seek.revision))
        } else {
            None
        };
        cancelled.arm(request_id);
        let cancellation_observed = cancelled.wait();
        let (pause_sequence, pause_revision) = seek_id
            .map(|(_, revision)| (4, revision))
            .unwrap_or((3, resumed.revision));
        let paused = player.submit(&f.actor, PlayerCommand {
            id: Uuid::new_v4(), lease: attached.lease, sequence: pause_sequence,
            revision: pause_revision, action: PlayerAction::Pause {},
        });
        let pause_receipt = tokio::time::timeout(Duration::from_secs(1), paused).await
            .map_err(|_| "Pause admission exceeded its one-second bound")?
            .map_err(|_| "Pause command was not admitted")?;
        tokio::time::timeout(Duration::from_secs(3), cancellation_observed).await
            .map_err(|_| "held attempt did not publish its cancelled completion")?;
        let paused_idle = (*tokio::time::timeout(Duration::from_secs(3), state.wait_for(|snapshot| {
            snapshot.revision >= pause_receipt.revision
                && snapshot.phase == PlayerPhase::Idle
                && snapshot.error.is_none()
                && !snapshot.playing
                && !snapshot.desired_playing
                && snapshot.last_failure.is_none()
                && snapshot.current.as_ref().is_some_and(|entry| entry.id == target_entry)
                && snapshot.selection == target_selection
        })).await.map_err(|_| "paused acquisition did not publish the idle state")?
            .map_err(|_| "authorized state observer closed before paused idle")?).clone();
        if paused_idle.current.as_ref().map(|entry| entry.id) != Some(target_entry) {
            return Err("paused idle update did not retain the target occurrence");
        }
        let replay = tokio::time::timeout(Duration::from_secs(3), f.services.catalog.resolve_source_until(
            &f.actor, &f.recording.to_string(), ResolveSource { request_id }, std::future::pending(),
        )).await.map_err(|_| "held resolution replay did not complete after Pause (expected a cancelled receipt)")?
            .map_err(|_| "held resolution replay returned a Core error instead of its receipt")?;
        if !matches!(replay.outcome, SourceResolutionOutcome::Cancelled) {
            return Err("held resolution replay completed without the expected cancelled receipt");
        }
        let immediately_paused = player.inspect(&f.actor).await.unwrap().state.unwrap();
        let immediately_paused_queue = player.queue(&f.actor, attached.lease,
            immediately_paused.revision, 0, 10).await.unwrap();
        let immediately_paused_entries: Value =
            serde_json::to_value(&immediately_paused_queue.entries).unwrap();
        if immediately_paused_entries != original_entries {
            return Err("Pause changed an authorized queue entry before held reply release");
        }
        second_gate.release();
        tokio::time::timeout(Duration::from_secs(3), second_gate.retired()).await
            .map_err(|_| "held search did not retire after release")?;
        let active_mark = monitor.as_ref().unwrap().mark();
        monitor.as_ref().unwrap().after(active_mark, 0.3);
        let active_silent = capture::energy(&monitor.as_ref().unwrap().recent(0.1)) < 0.000001;
        let after = player.inspect(&f.actor).await.unwrap().state.unwrap();
        let after_queue = player.queue(&f.actor, attached.lease, after.revision, 0, 10).await.unwrap();
        let after_entries: Value = serde_json::to_value(&after_queue.entries).unwrap();
        let paused_counts = (f.second.requests_for("v1/search").len(),
            f.second.requests_for("v1/recordings/detail").len(),
            f.second.requests_for("v1/sources").len(), f.second.requests_for("v1/audio").len());
        if !active_silent { return Err("native monitor was not silent during the paused acquisition drain"); }
        if after.current.as_ref().map(|current| current.id) != Some(target_entry)
            || after.selection != target_selection || after.current_index != Some(0)
            || after.phase != PlayerPhase::Idle || after.error.is_some()
            || after.desired_playing || after.playing || after.last_failure.is_some()
            || after_entries != original_entries
        { return Err("paused acquisition changed playback state or the authorized queue entries"); }
        if paused_counts != (2, 0, 0, 0) {
            return Err("paused acquisition issued detail, source, audio, or extra search requests");
        }
        if !f.protocol_clean() {
            return Err("the paused provider exchanges violated their protocol contract");
        }
        if scenario == Scenario::PauseThenResume {
            let (seek_id, _) = seek_id.ok_or("the resume control did not retain its seek identity")?;
            let mut position = player.subscribe_position(&f.actor, attached.lease).await.unwrap();
            player.submit(&f.actor, PlayerCommand {
                id: Uuid::new_v4(), lease: attached.lease, sequence: 5,
                revision: pause_receipt.revision, action: PlayerAction::Resume {},
            }).await.map_err(|_| "Resume command was not admitted")?;
            let first_progress = *tokio::time::timeout(Duration::from_secs(3), position.wait_for(|observed| {
                observed.selection == target_selection
                    && observed.entry == Some(target_entry)
                    && observed.seek_id == Some(seek_id)
                    && observed.position_ms > 0
            })).await.map_err(|_| "resumed target did not publish its seek-scoped position")?
                .map_err(|_| "authorized position observer closed after Resume")?;
            if first_progress.position_ms < 1000 {
                return Err("resumed target restarted before the retained 1000ms seek baseline");
            }
            let progressed = *tokio::time::timeout(Duration::from_secs(3), position.wait_for(|observed| {
                observed.selection == target_selection
                    && observed.entry == Some(target_entry)
                    && observed.seek_id == Some(seek_id)
                    && observed.position_ms >= first_progress.position_ms + 100
            })).await.map_err(|_| "resumed target did not progress beyond its seek baseline")?
                .map_err(|_| "authorized position observer closed during resumed playback")?;
            if progressed.position_ms < first_progress.position_ms + 100 {
                return Err("resumed target did not advance past the retained seek baseline");
            }
            let ready_state = tokio::time::timeout(Duration::from_secs(3), state.wait_for(|snapshot| {
                snapshot.selection == target_selection
                    && snapshot.current.as_ref().is_some_and(|entry| {
                        entry.id == target_entry && entry.recording_id == f.recording
                    })
                    && snapshot.current_index == Some(0)
                    && snapshot.phase == PlayerPhase::Ready
                    && snapshot.error.is_none()
                    && snapshot.desired_playing
                    && snapshot.playing
                    && snapshot.last_failure.is_none()
            })).await.map_err(|_| "resumed target did not become ready and playing")?
                .map_err(|_| "authorized state observer closed during resumed playback")?.clone();
            let after_resume = player.queue(&f.actor, attached.lease, ready_state.revision, 0, 10)
                .await.unwrap();
            let stable_original = original_queue.entries.iter().map(|entry| {
                (entry.id, entry.recording.id.clone(), entry.chapter)
            }).collect::<Vec<_>>();
            let stable_after = after_resume.entries.iter().map(|entry| {
                (entry.id, entry.recording.id.clone(), entry.chapter)
            }).collect::<Vec<_>>();
            if stable_after != stable_original {
                return Err("successful Resume changed queue occurrence identities or chapter anchors");
            }
            monitor.as_ref().unwrap().after(monitor.as_ref().unwrap().mark(), 0.1);
            let presented = monitor.as_ref().unwrap().recent(0.1);
            if capture::amplitude(&presented, 440.) <= 0.2
                || capture::amplitude(&presented, 880.) >= 0.02
            {
                return Err("resumed playback did not present only the owned 440Hz audio");
            }
            let resumed_counts = (f.second.requests_for("v1/search").len(),
                f.second.requests_for("v1/recordings/detail").len(),
                f.second.requests_for("v1/sources").len(), f.second.requests_for("v1/audio").len());
            if resumed_counts.0 != 3 || resumed_counts.1 != 1
                || resumed_counts.2 != 1 || resumed_counts.3 == 0
            {
                return Err("fresh Resume did not perform one provider search, hydration, source lookup, and audio request");
            }
            if !f.protocol_clean() {
                return Err("resumed provider exchanges violated their protocol contract");
            }
            let resumed_attempts = logs.events().iter()
                .filter(|event| event["fields"]["event"] == "player_source_discovery_attempt_started")
                .filter_map(|event| Some((
                    event["fields"]["attempt"].as_u64()?,
                    event["fields"]["selection"].as_u64()?,
                    Uuid::parse_str(event["fields"]["request_id"].as_str()?).ok()?,
                )))
                .filter(|(_, selection, _)| *selection == target_selection)
                .map(|(attempt, _, id)| (attempt, id))
                .collect::<Vec<_>>();
            let fresh_requests = resumed_attempts.iter()
                .filter(|(_, id)| *id != first_request && *id != request_id)
                .collect::<Vec<_>>();
            if resumed_attempts.len() != 3 || fresh_requests.len() != 1
                || fresh_requests[0].0 != 1 || fresh_requests[0].1.is_nil()
            {
                return Err("Resume did not create one new discovery request identity");
            }
        }
        retained = Some((target_entry, target_selection, active_silent, pause_receipt.revision));
        Ok::<(), &'static str>(())
    }).catch_unwind().await;

    let open_first = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if let Some(gate) = first_gate.as_ref() {
            gate.open();
        }
    }))
    .is_ok();
    let open_second = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if let Some(gate) = second_gate.as_ref() {
            gate.open();
        }
    }))
    .is_ok();
    let mut player_to_drop = None;
    let mut closing = None;
    let owner_joined = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if let Some((player, owner)) = native.take() {
            player_to_drop = Some(player);
            closing = Some(Box::pin(owner.close()));
        }
    }))
    .is_ok()
        && AssertUnwindSafe(async {
            let Some(close) = closing.as_mut() else {
                return true;
            };
            match tokio::time::timeout(Duration::from_secs(3), close.as_mut()).await {
                Ok(result) => result.is_ok(),
                Err(_) => {
                    let _ = close.as_mut().await;
                    false
                }
            }
        })
        .catch_unwind()
        .await
        .is_ok_and(|joined| joined);
    let close_future_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(closing.take()))).is_ok();
    let player_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(player_to_drop.take()))).is_ok();
    let witness_finished = std::panic::catch_unwind(AssertUnwindSafe(|| {
        witness.as_ref().map(OutputWitness::finish)
    }))
    .is_ok_and(|report| {
        matches!(
            (scenario, report),
            (_, Some(Ok(_)))
                | (
                    Scenario::PauseOnly,
                    None | Some(Err(WitnessError::Incomplete))
                )
        )
    });
    let witness_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(witness.take()))).is_ok();
    let post_silence = std::panic::catch_unwind(AssertUnwindSafe(|| {
        monitor.as_ref().is_none_or(|monitor| {
            monitor.after(monitor.mark(), 0.3);
            capture::energy(&monitor.recent(0.1)) < 0.000001
        })
    }))
    .is_ok_and(|silent| silent);
    let monitor_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(monitor.take()))).is_ok();
    let first_gate_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(first_gate.take()))).is_ok();
    let second_gate_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(second_gate.take()))).is_ok();
    let preserved = AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(3), f.preserved()).await
    })
    .catch_unwind()
    .await
    .is_ok_and(|result| result.is_ok_and(|preserved| preserved));
    let fixture_closed = AssertUnwindSafe(fixture::close(f))
        .catch_unwind()
        .await
        .is_ok_and(|closed| closed);

    logs.assert_excludes(
        &private_values
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    logs.assert_excludes(&[
        fixture::TITLE,
        fixture::ARTIST,
        fixture::PRIVATE_TITLE,
        fixture::TOKEN,
    ]);
    assert!(
        open_first && open_second && first_gate_dropped && second_gate_dropped,
        "owned gates must be opened and dropped"
    );
    assert!(
        owner_joined && close_future_dropped && player_dropped,
        "native owner close and player drop must complete"
    );
    assert!(
        witness_finished && witness_dropped && monitor_dropped && post_silence,
        "output callback witness and monitor cleanup must complete"
    );
    assert!(
        preserved && fixture_closed,
        "owned fixture state and services must retire cleanly"
    );
    match behavior {
        Ok(Ok(())) => {}
        Ok(Err(reason)) => panic!("Pause acquisition tracer failed: {reason}"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
    let (entry, selection, active_silent, revision) =
        retained.expect("the authorized post-drain observations must complete");
    assert!(
        !entry.is_nil() && selection > 0 && revision > 0 && active_silent,
        "the same target occurrence remained paused and silent through drain"
    );
}

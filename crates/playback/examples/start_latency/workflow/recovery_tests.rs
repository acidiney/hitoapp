use super::*;
use crate::{
    measurement::Measurements,
    resources::{OwnedRun, RunReport, SafeCode},
};
use futures_util::FutureExt;
use hito_core::{AdoptProviderRecording, Credentials, InstallPlugin, OfferPluginService};
use hito_playback::PlayerPhase;
use serde::Serialize;
use std::{
    panic::AssertUnwindSafe,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default, Serialize)]
struct PlayedCheckpoint {
    original_source: bool,
    same_recording_and_entry: bool,
    ready_and_playing: bool,
    selection: u64,
    position_selection: u64,
    position_ms: u64,
}

#[derive(Clone, Default, Serialize)]
struct Causality {
    reached_recovery: bool,
    oracle_accepted: bool,
    configured_sources: usize,
    open_streams_before_drop: Option<u64>,
    open_streams_after_drop: Option<u64>,
    provider_drop_epoch_ms: Option<f64>,
    public_source_unchanged: bool,
    no_replacement_source: bool,
    same_recording: bool,
    selection: u64,
    position_selection: u64,
    public_position_ms: u64,
    ready: bool,
    playing: bool,
    request_to_silence_ms: Option<f64>,
    request_to_audible_ms: Option<f64>,
    played_checkpoint: Option<PlayedCheckpoint>,
    replacement_source_changed: bool,
    independent_progress_after_return: bool,
    fresh_audible_read_after_independent_progress: bool,
}

struct ControlResult {
    witness: Causality,
    report: RunReport,
    released: bool,
    exported: bool,
}

async fn source_control(available_alternate: bool) -> ControlResult {
    let audio =
        std::env::var("HITO_BENCH_CONTROL_AUDIO").expect("owned recovery audio is required");
    let output = PathBuf::from(
        std::env::var("HITO_BENCH_CONTROL_OUT").expect("owned recovery output is required"),
    );
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let weak = Arc::downgrade(&directory);
    let evidence = Arc::new(Mutex::new(Causality::default()));
    let observed = evidence.clone();
    let measured = output.join("measurements");
    let report = OwnedRun::retain(directory).execute(move |run| Box::pin(async move {
        let music = run.directory.path().join("music");
        std::fs::create_dir(&music)?;
        run.services = Some(hito_core::open(run.config(music)?).await?);
        let services = run.services.as_ref().unwrap().clone();
        let session = services.identity.setup(Credentials {
            username: "recovery-control".into(), password: "owned recovery control password".into(),
        }).await?;
        let actor = services.identity.authenticate(&session.token).await?;
        run.provider = Some(ProviderProcess::start("/workspace/apps/web/e2e/latency/fake-provider.mjs", &audio, 1)?);
        let provider = run.provider.as_mut().unwrap();
        let name = "bench-remote-0";
        let a = "audio/bench-remote-0/a";
        let b = "audio/bench-remote-0/b";
        let configured: Vec<&str> = if available_alternate { vec![a, b] } else { vec![a] };
        provider.sources(name, &configured)?;
        provider.pace(a)?;
        if available_alternate { provider.pace(b)?; }
        let offered = services.plugins.offer_service(&actor, OfferPluginService {
            name: "Owned recovery control".into(), base_url: provider.address.clone(),
        }).await?;
        let installed = services.plugins.install(&actor, InstallPlugin {
            service_id: offered.id, service_revision: offered.revision,
            accept_manifest_digest: None, bearer_token: Some(provider.token.clone()),
        }).await?;
        let recording = services.catalog.adopt_provider(&actor, &installed.id, AdoptProviderRecording {
            installation_revision: installed.revision, provider_recording_id: name.into(),
        }).await?.id;
        let sources = services.catalog.sources(&actor, &recording).await?;
        if sources.is_empty() { return Err(SafeCode("control_source_missing").into()); }
        let expected_sources: Vec<_> = sources.iter().map(|source| source.id.clone()).collect();
        let recording_id = Uuid::parse_str(&recording)?;
        run.monitor = Some(Monitor::open()?);
        let (player, owner) = Player::native(services.clone());
        run.player_owner = Some(owner);
        let attached = player.claim(&actor, player.inspect(&actor).await?.epoch).await?;
        let state = player.subscribe(&actor, attached.lease).await?;
        let mut position = player.subscribe_position(&actor, attached.lease).await?;
        std::fs::create_dir(&measured)?;
        let samples = Measurements::open(&measured, 0)?;
        run.rig = Some(Rig::new(player, actor, attached.lease, state, run.monitor.take().unwrap(), samples));
        let rig = run.rig.as_mut().unwrap();
        let observed_state = rig.state.clone();
        let mut observed_position = position.clone();
        let (finished, stopped) = tokio::sync::oneshot::channel::<()>();
        let observe_played = AssertUnwindSafe(async move {
            tokio::select! {
                biased;
                result = observed_position.wait_for(|position| {
                    let state = observed_state.borrow();
                    position.position_ms >= 1500 && position.selection == state.selection
                        && state.current.is_some_and(|entry| entry.recording_id == recording_id && position.entry == Some(entry.id))
                        && state.source_id.as_ref().is_some_and(|source| expected_sources.contains(source))
                        && state.phase == PlayerPhase::Ready && state.playing
                }) => result.ok().map(|position| {
                    let state = observed_state.borrow();
                    (PlayedCheckpoint {
                        original_source: state.source_id.as_ref().is_some_and(|source| expected_sources.contains(source)),
                        same_recording_and_entry: state.current.is_some_and(|entry| entry.recording_id == recording_id && position.entry == Some(entry.id)),
                        ready_and_playing: state.phase == PlayerPhase::Ready && state.playing,
                        selection: state.selection, position_selection: position.selection, position_ms: position.position_ms,
                    }, state.source_id.clone(), state.current)
                }),
                _ = stopped => None,
            }
        }).catch_unwind();
        let recover = async {
            let result = AssertUnwindSafe(recovery_once(rig, provider, &Recovery {
                recording: &recording, name, a, b, refreshed: false, latency: 0,
            })).catch_unwind().await;
            let _ = finished.send(());
            result
        };
        let (result, played_checkpoint) = tokio::join!(recover, observe_played);
        let result = result.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        let played_checkpoint = played_checkpoint.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        let (played_checkpoint, original_source, original_entry) = match played_checkpoint {
            Some((played, source, entry)) => (Some(played), source, entry),
            None => (None, None, None),
        };
        let state = rig.state.borrow().clone();
        let at_return = *position.borrow();
        let replacement_source_changed = original_source.is_some() && state.source_id.as_ref().is_some_and(|source| !source.is_empty() && Some(source) != original_source.as_ref());
        let mut independent_progress_after_return = false;
        let mut fresh_audible_read_after_independent_progress = false;
        if available_alternate && replacement_source_changed && let (Ok((request, _, _)), Some(entry), Some(source)) = (&result, original_entry, state.source_id.as_deref()) {
            let selection = state.selection;
            let deadline = *request + Duration::from_secs(15);
            let progressed = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), position.wait_for(|position| {
                let latest = rig.state.borrow();
                position.position_ms > at_return.position_ms && position.entry == Some(entry.id) && position.selection == selection
                    && latest.current == Some(entry) && latest.selection == selection && latest.source_id.as_deref() == Some(source)
                    && latest.phase == PlayerPhase::Ready && latest.playing
            })).await;
            independent_progress_after_return = matches!(progressed, Ok(Ok(_))) && Instant::now() < deadline;
            drop(progressed);
            if independent_progress_after_return {
                let mark = Instant::now();
                loop {
                    if Instant::now() >= deadline { break; }
                    if rig.monitor.audible_read_after(mark)?.is_some() {
                        let latest = rig.state.borrow();
                        fresh_audible_read_after_independent_progress = Instant::now() < deadline && latest.current == Some(entry)
                            && latest.selection == selection && latest.source_id.as_deref() == Some(source)
                            && latest.phase == PlayerPhase::Ready && latest.playing;
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            }
        }
        let fault = provider.last_drop();
        *observed.lock().unwrap() = Causality {
            reached_recovery: true,
            oracle_accepted: result.is_ok(),
            configured_sources: sources.len(),
            open_streams_before_drop: fault.map(|fault| fault.before),
            open_streams_after_drop: fault.map(|fault| fault.after),
            provider_drop_epoch_ms: fault.map(|fault| fault.at_ms),
            public_source_unchanged: original_source.is_some() && state.source_id == original_source,
            no_replacement_source: state.source_id.as_ref().is_none_or(|source| Some(source) == original_source.as_ref()),
            same_recording: state.current.is_some_and(|entry| entry.recording_id == recording_id && at_return.entry == Some(entry.id) && Some(entry) == original_entry),
            selection: state.selection,
            position_selection: at_return.selection,
            public_position_ms: at_return.position_ms,
            ready: state.phase == PlayerPhase::Ready,
            playing: state.playing,
            request_to_silence_ms: result.as_ref().ok().and_then(|(request, silence, _)| silence.map(|at| at.duration_since(*request).as_secs_f64() * 1000.)),
            request_to_audible_ms: result.as_ref().ok().map(|(request, _, audible)| audible.duration_since(*request).as_secs_f64() * 1000.),
            played_checkpoint,
            replacement_source_changed,
            independent_progress_after_return,
            fresh_audible_read_after_independent_progress,
        };
        Ok(())
    })).await;
    let released = weak.upgrade().is_none();
    let evidence = evidence.lock().unwrap().clone();
    let exported = serde_json::json!({
        "scope": "actual source-causality control; provider epoch and monitor read clocks are separate",
        "available_alternate": available_alternate,
        "witness": &evidence, "work": report.work, "cleanup_failures": report.cleanup_failures,
        "sqlite_resources_retired": report.sqlite_resources_retired,
        "sqlite_deadline_exceeded": report.sqlite_deadline_exceeded, "storage_released": released,
    });
    let export = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("joined-result.json"));
    let exported = export
        .and_then(|mut file| {
            serde_json::to_writer_pretty(&mut file, &exported).map_err(std::io::Error::other)
        })
        .is_ok();
    ControlResult {
        witness: evidence,
        report,
        released,
        exported,
    }
}

fn assert_control_closed_and_played(control: &ControlResult, expected_sources: usize) {
    let ControlResult {
        witness: evidence,
        report,
        released,
        exported,
    } = control;
    assert!(*exported, "joined control evidence could not be exported");
    assert!(
        *released
            && report.sqlite_resources_retired
            && !report.sqlite_deadline_exceeded
            && report.cleanup_failures.is_empty(),
        "control resources did not close and retire"
    );
    assert!(
        evidence.reached_recovery && report.work == "complete",
        "control did not reach the recovery checkpoint"
    );
    assert!(
        evidence.configured_sources == expected_sources
            && evidence
                .open_streams_before_drop
                .is_some_and(|count| count > 0)
            && evidence.open_streams_after_drop == Some(0),
        "control did not withdraw a live configured stream"
    );
    assert!(
        evidence
            .played_checkpoint
            .as_ref()
            .is_some_and(|played| played.original_source
                && played.same_recording_and_entry
                && played.ready_and_playing
                && played.selection == played.position_selection
                && played.position_ms >= 1500),
        "control did not observe the played public selection"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor and owned recovery control MP3"]
async fn old_source_content_gap_cannot_prove_successful_recovery() {
    let control = source_control(false).await;
    assert_control_closed_and_played(&control, 1);
    assert!(
        control.witness.no_replacement_source,
        "control gained a replacement source"
    );
    assert!(
        !control.witness.oracle_accepted,
        "old-source musical silence and buffered tone were accepted as recovery without a replacement source"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor and owned recovery control MP3"]
async fn actual_alternate_recovery_has_new_source_progress_and_fresh_monitor_audio() {
    let control = source_control(true).await;
    assert_control_closed_and_played(&control, 2);
    let evidence = control.witness;
    assert!(
        evidence.oracle_accepted
            && evidence.replacement_source_changed
            && evidence.same_recording
            && evidence.selection == evidence.position_selection
            && evidence.ready
            && evidence.playing,
        "alternate recovery returned without a matching changed source"
    );
    assert!(
        evidence.independent_progress_after_return
            && evidence.fresh_audible_read_after_independent_progress,
        "alternate recovery did not produce fresh public progress and monitor audio"
    );
}

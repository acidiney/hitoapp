#![cfg(feature = "device")]

#[path = "../fixtures/occurrence_retry.rs"]
mod fixture;

use futures_util::FutureExt;
use hito_audio::test_support::{
    capture,
    output_witness::{OutputWitness, WitnessError},
};
use hito_core::test_support::hang_guard::hang_guard;
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use hito_testkit::logs::LogCapture;
use std::{collections::HashSet, panic::AssertUnwindSafe, time::Duration};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn three_fresh_discovery_attempts_present_the_first_occurrence_without_skipping() {
    let f = fixture::Fixture::start().await;
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    exercise(f, &logs).with_subscriber(dispatch).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn three_no_suitable_passes_advance_once_to_the_audible_sentinel() {
    let f = fixture::Fixture::start_no_suitable().await;
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    three_no_suitable(f, &logs).with_subscriber(dispatch).await;
}

async fn three_no_suitable(f: fixture::Fixture, logs: &LogCapture) {
    let witness = OutputWitness::begin(48_000 * 20, 4096).unwrap();
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.services.clone());
    let attached = hang_guard("owned negative acquisition claim", async {
        player
            .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
            .await
            .unwrap()
    })
    .await;
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
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
    let observed = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            async {
                state
                    .wait_for(|state| state.selection == 2 && state.last_failure.is_some())
                    .await
                    .map(|value| (*value).clone())
            },
            async {
                position
                    .wait_for(|position| position.selection == 2 && position.position_ms > 100)
                    .await
                    .map(|value| *value)
            },
        )
    })
    .await;
    let final_state = player.inspect(&f.actor).await.unwrap().state.unwrap();
    let sentinel_id = f.sentinel;
    let final_position = *position.borrow();
    capture.after(capture.mark(), 0.1);
    let private_audio = capture.recent(0.1);
    let first_order = f.order.lock().unwrap().clone();
    let first_searches = f.first.requests_for("v1/search").len();
    let second_searches = f.second.requests_for("v1/search").len();
    let hydration = [&f.first, &f.second].iter().any(|provider| {
        ["v1/recordings/detail", "v1/sources", "v1/audio"]
            .iter()
            .any(|path| !provider.requests_for(path).is_empty())
    });
    let protocol_clean = f.protocol_clean();
    let private_values = f.private_values();
    let final_logs = logs.events();
    let owner_joined = owner.close().await.is_ok();
    drop(player);
    let output = witness.finish();
    let output_joined = !matches!(output, Err(WitnessError::ActiveWriters));
    let silent = {
        capture.after(capture.mark(), 0.3);
        capture::energy(&capture.recent(0.1)) < 0.000001
    };
    drop(capture);
    let preserved = f.preserved_target_and_files().await;
    let joined = f.close().await;

    assert!(
        owner_joined && joined,
        "all owned player and Core work must join"
    );
    assert!(
        output_joined,
        "output callback writers must join before the witness is read"
    );
    assert!(
        silent,
        "joined output must leave the private monitor silent"
    );
    assert!(
        protocol_clean,
        "all fixture exchanges must satisfy the SDK contract"
    );
    assert!(
        preserved,
        "target metadata and both owned audio files must be preserved"
    );
    logs.assert_excludes(&[
        fixture::TITLE,
        fixture::ARTIST,
        fixture::PRIVATE_TITLE,
        fixture::TOKEN,
    ]);
    for value in &private_values {
        logs.assert_excludes(&[value]);
    }
    let selected_current_count = final_logs
        .iter()
        .filter(|event| event["fields"]["event"] == "player_source_selected")
        .filter(|event| {
            final_state
                .source_id
                .as_deref()
                .is_some_and(|source_id| event["fields"]["source_id"].as_str() == Some(source_id))
        })
        .count();
    let output_changes = final_logs
        .iter()
        .filter(|event| event["fields"]["event"] == "player_output_changed")
        .map(|event| {
            (
                event["fields"]["selection"].as_u64(),
                event["fields"]["phase"].as_str(),
                event["fields"]["playing"].as_bool(),
                event["fields"]["ended"].as_bool(),
                event["fields"]["error"].as_str(),
            )
        })
        .collect::<Vec<_>>();
    let session_ready_count = final_logs
        .iter()
        .filter(|event| event["fields"]["event"] == "native_session_ready")
        .count();
    let refreshes = final_logs
        .iter()
        .filter(|event| event["fields"]["event"] == "player_sources_refreshed")
        .map(|event| event["fields"]["success"].as_bool())
        .collect::<Vec<_>>();
    let native_close_failures = final_logs
        .iter()
        .filter(|event| event["fields"]["event"] == "player_native_close_failed")
        .count();
    let diagnostic = format!(
        "observed={}; selection={}; index={:?}; sentinel_current={}; desired_playing={}; phase={:?}; playing={}; source_present={}; error={:?}; last_failure={:?}; position={}/{}; position_entry_matches_current={}; selected_current_count={}; session_ready_count={}; refreshes={:?}; close_failures={}; searches={}/{}; order={:?}; resolution_outcomes={:?}; native_failures={:?}; output_changes={:?}",
        observed.is_ok(),
        final_state.selection,
        final_state.current_index,
        final_state
            .current
            .as_ref()
            .is_some_and(|entry| entry.recording_id == sentinel_id),
        final_state.desired_playing,
        final_state.phase,
        final_state.playing,
        final_state.source_id.is_some(),
        final_state.error,
        final_state.last_failure,
        final_position.selection,
        final_position.position_ms,
        final_state.current.as_ref().map(|entry| entry.id) == final_position.entry,
        selected_current_count,
        session_ready_count,
        refreshes,
        native_close_failures,
        first_searches,
        second_searches,
        first_order,
        final_logs
            .iter()
            .filter(|event| event["fields"]["event"] == "source_resolution_finished")
            .map(|event| event["fields"]["outcome"].as_str().unwrap_or("missing"))
            .collect::<Vec<_>>(),
        final_logs
            .iter()
            .filter(|event| event["fields"]["event"] == "player_source_failed")
            .map(|event| event["fields"]["reason"].as_str().unwrap_or("missing"))
            .collect::<Vec<_>>(),
        output_changes,
    );
    assert!(
        observed.is_ok(),
        "three negative passes must reach sentinel; {diagnostic}"
    );
    let (state_result, position_result) = observed
        .expect("three negative passes must reach the sentinel within five seconds after cleanup");
    state_result.expect("the authorized state observer must remain open");
    let presented = position_result.expect("the authorized position observer must remain open");
    assert!(
        output.is_ok(),
        "the sentinel must produce a complete output witness"
    );
    assert_eq!(presented.selection, 2);
    assert_eq!(final_state.selection, 2);
    assert_eq!(final_state.current_index, Some(1));
    assert_eq!(
        final_state.current.as_ref().map(|entry| entry.recording_id),
        Some(sentinel_id)
    );
    assert!(
        final_state.last_failure.is_some(),
        "the exhausted target failure must be retained once"
    );
    assert_eq!(first_searches, 3);
    assert_eq!(second_searches, 3);
    assert_eq!(first_order, vec!['A', 'B', 'A', 'B', 'A', 'B']);
    assert!(
        !hydration,
        "NoSuitable must not hydrate provider details, sources or audio"
    );
    let resolutions = final_logs
        .iter()
        .filter(|event| event["fields"]["event"] == "source_resolution_finished")
        .collect::<Vec<_>>();
    assert_eq!(resolutions.len(), 3);
    assert_eq!(
        resolutions
            .iter()
            .map(|event| event["fields"]["outcome"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "no_suitable_source",
            "no_suitable_source",
            "no_suitable_source"
        ]
    );
    let ids = resolutions
        .iter()
        .map(|event| event["fields"]["resolution_id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(ids.len(), 3);
    let acquisition_starts = final_logs
        .iter()
        .filter(|event| event["fields"]["event"] == "player_acquisition_started")
        .collect::<Vec<_>>();
    let attempts = final_logs
        .iter()
        .filter(|event| event["fields"]["event"] == "player_source_discovery_attempt_started")
        .map(|event| {
            (
                event["fields"]["operation_id"].as_str().unwrap(),
                event["fields"]["attempt"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        attempts
            .iter()
            .map(|(_, attempt)| *attempt)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    let target_demand = attempts[0].0;
    assert!(
        attempts
            .iter()
            .all(|(request_id, _)| *request_id == target_demand)
    );
    assert_eq!(
        acquisition_starts
            .iter()
            .filter(|event| event["fields"]["request_id"] == target_demand)
            .count(),
        1,
        "the target must have one acquisition start while allowing sentinel acquisition"
    );
    assert!(capture::amplitude(&private_audio, 880.) > 0.2);
    assert!(capture::amplitude(&private_audio, 440.) < 0.02);
}

async fn exercise(f: fixture::Fixture, logs: &LogCapture) {
    let witness = OutputWitness::begin(48_000 * 20, 4096).unwrap();
    let capture = capture::Capture::open();
    let first_search = f.second.hold_next("v1/search");
    let (player, owner) = Player::native(f.services.clone());
    let behavior = AssertUnwindSafe(async {
        let attached = hang_guard("owned occurrence claim", async {
            player.claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
                .await.unwrap()
        }).await;
        let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
        let mut position = player.subscribe_position(&f.actor, attached.lease).await.unwrap();
        let queued = player.submit(&f.actor, PlayerCommand {
            id: Uuid::new_v4(), lease: attached.lease, sequence: 1, revision: 0,
            action: PlayerAction::Replace { recordings: vec![f.recording, f.sentinel], start: 0 },
        }).await.unwrap();
        let initial = player.inspect(&f.actor).await.unwrap().state.unwrap();
        let first_entry = initial.current.unwrap().id;
        player.submit(&f.actor, PlayerCommand {
            id: Uuid::new_v4(), lease: attached.lease, sequence: 2, revision: queued.revision,
            action: PlayerAction::Resume {},
        }).await.unwrap();
        let reached = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::select! {
                biased;
                _ = first_search.entered() => true,
                _ = state.wait_for(|state| state.selection != 1 || state.last_failure.is_some()) => false,
            }
        }).await.map_err(|_| "native acquisition never reached discovery within the existing bound")?;
        if !reached {
            return Err("native acquisition failed or skipped before its first automatic discovery");
        }
        f.rank(true).await;
        first_search.release();
        let progressed = *(tokio::time::timeout(Duration::from_secs(3),
            position.wait_for(|position| position.selection != 1 || position.position_ms >= 100))
            .await.map_err(|_| "the first occurrence never presented progressing audio")?
            .map_err(|_| "the authorized position observer closed during acquisition")?);
        if progressed.selection != 1 {
            return Err("the player skipped the requested occurrence during discovery retries");
        }
        let observed = player.inspect(&f.actor).await.unwrap().state.unwrap();
        if observed.selection != 1 || observed.current_index != Some(0)
            || observed.current.as_ref().is_none_or(|entry|
                entry.id != first_entry || entry.recording_id != f.recording)
            || observed.phase != PlayerPhase::Ready || !observed.playing
            || observed.last_failure.is_some()
        {
            return Err("progress did not belong to the unchanged first queue occurrence");
        }
        capture.after(capture.mark(), 0.1);
        let presented = capture.recent(0.1);
        if capture::amplitude(&presented, 440.) <= 0.2
            || capture::amplitude(&presented, 880.) >= 0.02
        {
            return Err("the private monitor did not present the requested owned audio alone");
        }
        Ok(())
    }).catch_unwind().await;

    let final_state = player.inspect(&f.actor).await;
    first_search.open();
    let mut closing = Box::pin(owner.close());
    let owner_join = tokio::time::timeout(Duration::from_secs(3), &mut closing).await;
    let owner_joined = match owner_join {
        Ok(result) => result.is_ok(),
        Err(_) => {
            let _ = closing.as_mut().await;
            false
        }
    };
    drop(closing);
    drop(player);
    let output = witness.finish();
    let output_joined = !matches!(output, Err(WitnessError::ActiveWriters));
    let silent = AssertUnwindSafe(async {
        capture.after(capture.mark(), 0.3);
        capture::energy(&capture.recent(0.1)) < 0.000001
    })
    .catch_unwind()
    .await;
    drop(capture);
    let preserved = AssertUnwindSafe(f.preserved()).catch_unwind().await;
    let order = f.order.lock().unwrap().clone();
    let searches = f.second.requests_for("v1/search").len();
    let details = f.second.requests_for("v1/recordings/detail").len();
    let sources = f.second.requests_for("v1/sources").len();
    let audio_requests = f.second.requests_for("v1/audio").len();
    let protocol_clean = f.protocol_clean();
    let private_values = f.private_values();
    let joined = f.close().await;

    let final_selection = final_state
        .ok()
        .and_then(|inspection| inspection.state)
        .map(|state| state.selection);
    tracing::info!(
        event = "native_occurrence_retry_tracer_observed",
        search_calls = searches,
        final_selection,
        owner_joined,
        output_joined,
        core_and_providers_joined = joined,
        "Owned occurrence tracer completed cleanup"
    );
    assert!(
        owner_joined && joined,
        "the player, provider and owned Core resources must retire"
    );
    assert!(
        output_joined,
        "native output callback writers must join before witness finish"
    );
    assert!(
        matches!(silent, Ok(true)),
        "joined output must leave the private monitor silent"
    );
    assert!(
        protocol_clean,
        "every fixture exchange must satisfy the approved SDK contract"
    );
    logs.assert_excludes(&[
        fixture::TITLE,
        fixture::ARTIST,
        fixture::PRIVATE_TITLE,
        fixture::TOKEN,
    ]);
    for value in &private_values {
        logs.assert_excludes(&[value]);
    }
    match behavior {
        Ok(Ok(())) => {}
        Ok(Err(reason)) => {
            panic!("{reason}; discovery_calls={searches}; final_selection={final_selection:?}")
        }
        Err(panic) => std::panic::resume_unwind(panic),
    }
    assert!(
        output.is_ok(),
        "successful playback must leave a complete output witness"
    );
    let output = output.unwrap();
    assert!(
        matches!(preserved, Ok(true)),
        "retrying must preserve files, primary metadata, likes and the unplayed sentinel cursor"
    );
    assert_eq!(
        order,
        vec!['A', 'B', 'B', 'A', 'B'],
        "every fresh discovery must restart in the listener's current provider order"
    );
    assert!(
        searches == 3 && details == 1 && sources == 1 && audio_requests > 0,
        "three fresh logical discoveries must precede one strict candidate hydration and actual audio"
    );
    let resolutions = logs
        .events()
        .into_iter()
        .filter(|event| event["fields"]["event"] == "source_resolution_finished")
        .collect::<Vec<_>>();
    assert_eq!(
        resolutions.len(),
        3,
        "three completed logical resolutions must be observable"
    );
    let ids = resolutions
        .iter()
        .map(|event| event["fields"]["resolution_id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(
        ids.len(),
        3,
        "completed negative receipts must never replace fresh logical work"
    );
    let outcomes = resolutions
        .iter()
        .map(|event| event["fields"]["outcome"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes,
        ["no_suitable_source", "no_suitable_source", "ready"]
    );
    assert!(
        !output.callbacks.is_empty() && capture::energy(&output.samples) > 0.00001,
        "the real native callback must contain nonzero owned audio"
    );
}

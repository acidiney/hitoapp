#![cfg(feature = "device")]

#[path = "fixtures/occurrence_retry_transient.rs"]
mod fixture;

use futures_util::FutureExt;
use hito_audio::test_support::{
    capture,
    output_witness::{OutputWitness, WitnessError},
};
use hito_core::{ResolveSource, test_support::hang_guard::hang_guard};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerPhase};
use hito_testkit::logs::LogCapture;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    panic::AssertUnwindSafe,
    time::{Duration, Instant},
};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_retryable_provider_outage_recovers_the_same_occurrence_without_skipping() {
    let f = fixture::start().await;
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    exercise(f, &logs).with_subscriber(dispatch).await;
}

async fn exercise(f: fixture::Fixture, logs: &LogCapture) {
    let mut witness = None;
    let mut capture = None;
    let mut first_search = None;
    let mut native = None;
    let mut acquisition_started = None;
    let behavior = AssertUnwindSafe(async {
        witness = Some(OutputWitness::begin(48_000 * 20, 4096).unwrap());
        capture = Some(capture::Capture::open());
        first_search = Some(f.second.hold_next("v1/search"));
        native = Some(Player::native(f.services.clone()));
        let (player, _) = native.as_ref().unwrap();
        let capture = capture.as_ref().unwrap();
        let first_search = first_search.as_ref().unwrap();
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
        acquisition_started = Some(Instant::now());
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
        first_search.release();
        let progressed = *tokio::time::timeout(Duration::from_secs(3),
            position.wait_for(|position| position.selection != 1 || position.position_ms >= 100))
            .await.map_err(|_| "the first occurrence never presented progressing audio")?
            .map_err(|_| "the authorized position observer closed during acquisition")?;
        if progressed.selection != 1 || progressed.entry != Some(first_entry) {
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
        let source = observed.source_id.ok_or("Ready playback has no authorized source identity")?;
        Ok((source,progressed.position_ms,progressed.duration_ms))
    }).catch_unwind().await;
    let acquisition_elapsed = acquisition_started.map(|started| started.elapsed());

    let native_created = native.is_some();
    let final_state = AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            match native.as_ref() {
                Some((player, _)) => player
                    .inspect(&f.actor)
                    .await
                    .ok()
                    .and_then(|inspection| inspection.state)
                    .map(|state| state.selection),
                None => None,
            }
        })
        .await
    })
    .catch_unwind()
    .await;
    let gate_opened = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if let Some(gate) = first_search.as_ref() {
            gate.open()
        }
    }))
    .is_ok();
    let (mut player, mut closing) = match native.take() {
        Some((player, owner)) => (Some(player), Some(Box::pin(owner.close()))),
        None => (None, None),
    };
    let owner_joined = AssertUnwindSafe(async {
        let Some(closing) = closing.as_mut() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(3), closing.as_mut()).await {
            Ok(result) => result.is_ok(),
            Err(_) => {
                let _ = closing.as_mut().await;
                false
            }
        }
    })
    .catch_unwind()
    .await
    .is_ok_and(|joined| joined);
    let closing_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(closing.take()))).is_ok();
    let player_dropped = std::panic::catch_unwind(AssertUnwindSafe(|| drop(player.take()))).is_ok();
    let gate_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(first_search.take()))).is_ok();
    let output = std::panic::catch_unwind(AssertUnwindSafe(|| {
        witness.as_ref().map(OutputWitness::finish)
    }));
    let output_joined = matches!(&output, Ok(None) | Ok(Some(Ok(_))))
        || matches!(&output,Ok(Some(Err(error))) if error!=&WitnessError::ActiveWriters);
    let witness_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(witness.take()))).is_ok();
    let silent = std::panic::catch_unwind(AssertUnwindSafe(|| {
        capture.as_ref().is_none_or(|capture| {
            capture.after(capture.mark(), 0.3);
            capture::energy(&capture.recent(0.1)) < 0.000001
        })
    }));
    let capture_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(capture.take()))).is_ok();
    let preserved = AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(3), f.preserved()).await
    })
    .catch_unwind()
    .await;
    let observations = std::panic::catch_unwind(AssertUnwindSafe(|| observe(&f)));
    let receipts = AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(3), read_receipts(&f, logs)).await
    })
    .catch_unwind()
    .await;
    let after_replay = std::panic::catch_unwind(AssertUnwindSafe(|| observe(&f)));
    let joined = AssertUnwindSafe(fixture::close(f))
        .catch_unwind()
        .await
        .is_ok_and(|joined| joined);
    let final_selection = match &final_state {
        Ok(Ok(selection)) => *selection,
        _ => None,
    };
    let first_searches = observations
        .as_ref()
        .ok()
        .map(|observed| observed.searches[0])
        .unwrap_or_default();
    let searches = observations
        .as_ref()
        .ok()
        .map(|observed| observed.searches[1])
        .unwrap_or_default();
    tracing::info!(
        event = "native_transient_occurrence_tracer_observed",
        acquisition_elapsed_ms = acquisition_elapsed.map(|elapsed| elapsed.as_millis() as u64),
        search_calls = searches,
        ordered_first_provider_calls = first_searches,
        final_selection,
        native_created,
        gate_opened,
        gate_dropped,
        owner_joined,
        player_dropped,
        witness_dropped,
        capture_dropped,
        output_joined,
        core_and_providers_joined = joined,
        "Owned occurrence tracer completed cleanup"
    );
    let observed = match observations {
        Ok(observed) => observed,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    let after_replay = match after_replay {
        Ok(observed) => observed,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    let order = observed.order;
    let details = observed.details;
    let sources = observed.sources;
    let audio_requests = observed.audio;
    let private_values = observed.private_values;
    let protocol_clean = observed.protocol_clean && after_replay.protocol_clean;
    let provider_counts_before_replay = observed.counts;
    let provider_counts_after_replay = after_replay.counts;
    assert!(
        final_state.is_ok_and(|result| result.is_ok()),
        "the final authorized inspection must complete"
    );
    assert!(
        gate_opened
            && gate_dropped
            && closing_dropped
            && player_dropped
            && witness_dropped
            && capture_dropped,
        "each owned playback cleanup stage must finish without panicking"
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
    let (presented_source, presented_position, presented_duration) = match behavior {
        Ok(Ok(observed)) => observed,
        Ok(Err(reason)) => {
            panic!("{reason}; discovery_calls={searches}; final_selection={final_selection:?}")
        }
        Err(panic) => std::panic::resume_unwind(panic),
    };
    let receipts = match receipts {
        Ok(Ok(Ok(receipts))) => receipts,
        Ok(Ok(Err(reason))) => panic!("{reason}"),
        Ok(Err(_)) => panic!("public receipt replay exceeded its existing behavior bound"),
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(acquisition_elapsed.is_some_and(|elapsed| elapsed < Duration::from_secs(15)));
    assert!(presented_position >= 100);
    assert_eq!(presented_duration, Some(8000));
    assert_eq!(
        provider_counts_after_replay, provider_counts_before_replay,
        "accepted receipt replay must not exchange with providers"
    );
    assert!(
        matches!(&output, Ok(Some(Ok(_)))),
        "successful playback must leave a complete output witness"
    );
    let output = match output {
        Ok(Some(Ok(output))) => output,
        Err(panic) => std::panic::resume_unwind(panic),
        _ => unreachable!(),
    };
    assert!(
        matches!(preserved, Ok(Ok(true))),
        "retrying must preserve files, primary metadata, likes and the unplayed sentinel cursor"
    );
    assert_eq!(
        order,
        vec!['A', 'B', 'A', 'B'],
        "every fresh discovery must restart in the listener's current provider order"
    );
    assert!(
        searches == 2 && first_searches == 2 && details == 1 && sources == 1 && audio_requests > 0,
        "a transient first search must precede one fresh logical retry, strict hydration and actual audio"
    );
    assert_eq!(
        receipts.started.len(),
        2,
        "two logical discovery attempts must be observable"
    );
    let request_ids = receipts
        .started
        .iter()
        .map(|event| event["fields"]["request_id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(
        request_ids.len(),
        2,
        "a retry must use a fresh nonnil request UUID"
    );
    let operations = receipts
        .started
        .iter()
        .map(|event| event["fields"]["operation_id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(
        operations.len(),
        1,
        "both tries must belong to the same occurrence operation"
    );
    for (index, event) in receipts.started.iter().enumerate() {
        assert_eq!(event["fields"]["attempt"], json!(index + 1));
        assert_eq!(event["fields"]["selection"], json!(1));
    }
    assert_eq!(receipts.attempts.len(), 2);
    for (index, event) in receipts.attempts.iter().enumerate() {
        assert_eq!(event["fields"]["attempt"], json!(index + 1));
        assert_eq!(event["fields"]["selection"], json!(1));
        assert_eq!(
            event["fields"]["request_id"],
            receipts.started[index]["fields"]["request_id"]
        );
        assert!(event["fields"]["elapsed_ms"].is_u64());
        assert!(event["fields"]["total_elapsed_ms"].as_u64().unwrap() < 15_000);
    }
    assert_eq!(
        receipts.attempts[0]["fields"]["reason"],
        json!("provider_unavailable")
    );
    assert_eq!(
        receipts.attempts[0]["fields"]["outcome"],
        json!("provider_unavailable")
    );
    assert_eq!(receipts.attempts[1]["fields"]["reason"], json!("ready"));
    assert_eq!(receipts.attempts[1]["fields"]["outcome"], json!("ready"));
    assert!(
        receipts.attempts[1]["fields"]["total_elapsed_ms"]
            .as_u64()
            .unwrap()
            >= 500
    );
    assert_eq!(receipts.finished.len(), 2);
    let ids = receipts
        .finished
        .iter()
        .map(|event| event["fields"]["resolution_id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(
        ids.len(),
        2,
        "each logical try must own a distinct completed receipt"
    );
    assert_eq!(
        receipts
            .finished
            .iter()
            .map(|event| event["fields"]["outcome"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["provider_unavailable", "ready"]
    );
    assert_eq!(receipts.responses.len(), 2);
    let failed = &receipts.responses[0];
    let ready = &receipts.responses[1];
    assert_eq!(failed["outcome"], json!("provider_unavailable"));
    assert_eq!(
        failed["failure"],
        json!({"reason":"provider_unavailable","retryable":true})
    );
    assert_eq!(
        failed, &receipts.failed_replay,
        "the first accepted failure decision must remain immutable after recovery"
    );
    assert_eq!(ready["outcome"], json!("ready"));
    assert_eq!(ready["failure"], Value::Null);
    for (index, response) in receipts.responses.iter().enumerate() {
        assert_eq!(
            response["request_id"],
            receipts.started[index]["fields"]["request_id"]
        );
        assert_eq!(
            response["resolution_id"],
            receipts.finished[index]["fields"]["resolution_id"]
        );
    }
    assert!(
        ready["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |source| source["id"].as_str() == Some(presented_source.as_str())
                    && source["ready"] == json!(true)
            ),
        "the actual presented source must remain a publicly authorized Ready source"
    );
    let events = logs.events();
    let unavailable_statuses = events
        .iter()
        .filter(|event| {
            event["fields"]["operation"] == json!("v1/search")
                && event["fields"]["status"] == json!(503)
        })
        .count();
    assert_eq!(
        unavailable_statuses, 1,
        "one actual provider exchange must return503"
    );
    assert!(
        !output.callbacks.is_empty() && capture::energy(&output.samples) > 0.00001,
        "the real native callback must contain nonzero owned audio"
    );
}

struct Observation {
    order: Vec<char>,
    counts: [usize; 2],
    searches: [usize; 2],
    details: usize,
    sources: usize,
    audio: usize,
    protocol_clean: bool,
    private_values: Vec<String>,
}

fn observe(f: &fixture::Fixture) -> Observation {
    Observation {
        order: f.order.lock().unwrap().clone(),
        counts: [f.first.requests().len(), f.second.requests().len()],
        searches: [
            f.first.requests_for("v1/search").len(),
            f.second.requests_for("v1/search").len(),
        ],
        details: f.second.requests_for("v1/recordings/detail").len(),
        sources: f.second.requests_for("v1/sources").len(),
        audio: f.second.requests_for("v1/audio").len(),
        protocol_clean: f.protocol_clean(),
        private_values: f.private_values(),
    }
}

struct Receipts {
    started: Vec<Value>,
    finished: Vec<Value>,
    attempts: Vec<Value>,
    responses: Vec<Value>,
    failed_replay: Value,
}

async fn read_receipts(f: &fixture::Fixture, logs: &LogCapture) -> Result<Receipts, &'static str> {
    let events = logs.events();
    let started = events
        .iter()
        .filter(|event| event["fields"]["event"] == "player_source_discovery_attempt_started")
        .cloned()
        .collect::<Vec<_>>();
    let attempts = events
        .iter()
        .filter(|event| event["fields"]["event"] == "player_source_discovery_attempt_finished")
        .cloned()
        .collect::<Vec<_>>();
    let finished = events
        .into_iter()
        .filter(|event| event["fields"]["event"] == "source_resolution_finished")
        .collect::<Vec<_>>();
    let mut responses = Vec::new();
    let mut requests = Vec::new();
    for event in &started {
        let request = event["fields"]["request_id"]
            .as_str()
            .ok_or("discovery did not publish a safe request identity")?;
        let request =
            Uuid::parse_str(request).map_err(|_| "discovery request identity is not a UUID")?;
        if request.is_nil() {
            return Err("discovery must not use a nil request identity");
        }
        let response = f
            .services
            .catalog
            .resolve_source_until(
                &f.actor,
                &f.recording.to_string(),
                ResolveSource {
                    request_id: request,
                },
                std::future::pending(),
            )
            .await
            .map_err(|_| "the owned public receipt could not be replayed")?;
        requests.push(request);
        responses.push(
            serde_json::to_value(response)
                .map_err(|_| "the public receipt could not be serialized")?,
        );
    }
    let first = requests
        .first()
        .ok_or("native playback never started a logical resolution")?;
    let replay = f
        .services
        .catalog
        .resolve_source_until(
            &f.actor,
            &f.recording.to_string(),
            ResolveSource { request_id: *first },
            std::future::pending(),
        )
        .await
        .map_err(|_| "the accepted failure receipt could not be replayed again")?;
    let failed_replay = serde_json::to_value(replay)
        .map_err(|_| "the accepted failure replay could not be serialized")?;
    Ok(Receipts {
        started,
        finished,
        attempts,
        responses,
        failed_replay,
    })
}

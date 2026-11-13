#![cfg(feature = "device")]

#[path = "fixtures/occurrence_retry_transient.rs"]
mod fixture;

use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_audio::test_support::{
    capture,
    output_witness::{OutputWitness, WitnessError},
};
use hito_core::{ResolveSource, test_support::hang_guard::hang_guard};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerError, PlayerPhase};
use hito_plugin_sdk::SearchRequest;
use hito_testkit::{fake_plugin::Reply, logs::LogCapture};
use serde_json::json;
use std::{
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_terminal_provider_refusal_advances_once_to_the_next_playable_occurrence() {
    let f = fixture::start().await;
    let searches = Arc::new(AtomicUsize::new(0));
    let order = f.order.clone();
    let search_count = searches.clone();
    let artist = fixture::ARTIST;
    let title = fixture::TITLE;
    f.second.on("v1/search", move |request| {
        let valid = request.query.is_none()
            && serde_json::from_slice::<SearchRequest>(&request.body).is_ok_and(|request| {
                request.validate().is_ok()
                    && request.query == format!("{artist} {title}")
                    && request.cursor.is_none()
                    && request.limit == 20
            });
        if !valid || search_count.fetch_add(1, Ordering::SeqCst) != 0 {
            return Reply::Status(StatusCode::BAD_REQUEST);
        }
        order.lock().unwrap().push('B');
        Reply::Raw {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            headers: HeaderMap::from_iter([(
                "content-type".parse().unwrap(),
                "application/json".parse().unwrap(),
            )]),
            body: br#"{"error":"provider refused this recording"}"#.to_vec(),
            chunked: false,
        }
    });
    let logs = LogCapture::default();
    let dispatch = logs.dispatch("off,hito_core=debug,hito_playback=debug");
    exercise(f, searches, &logs).with_subscriber(dispatch).await;
}

async fn exercise(f: fixture::Fixture, searches: Arc<AtomicUsize>, logs: &LogCapture) {
    let target_path = f.directory.path().join("owned-provider.wav");
    let sentinel_path = f.directory.path().join("music/sentinel.wav");
    let target_file_before = std::fs::read(&target_path).unwrap();
    let sentinel_file_before = std::fs::read(&sentinel_path).unwrap();
    let target_mtime_before = std::fs::metadata(&target_path).unwrap().modified().unwrap();
    let sentinel_mtime_before = std::fs::metadata(&sentinel_path)
        .unwrap()
        .modified()
        .unwrap();
    let target_before = f
        .services
        .catalog
        .recordings_by_ids(&f.actor, &[f.recording])
        .await
        .unwrap();
    let sources_before = f
        .services
        .catalog
        .sources(&f.actor, &f.recording.to_string())
        .await
        .unwrap();
    let preferences_before = f
        .services
        .catalog
        .source_preferences(&f.actor)
        .await
        .unwrap();
    let mut witness = None;
    let mut capture = None;
    let mut native = None;
    let mut acquisition_started = None;
    let started = Instant::now();
    let behavior = AssertUnwindSafe(async {
        witness = Some(OutputWitness::begin(48_000 * 20, 4096).unwrap());
        capture = Some(capture::Capture::open());
        native = Some(Player::native(f.services.clone()));
        let (player, _) = native.as_ref().unwrap();
        let capture = capture.as_ref().unwrap();
        let attached = hang_guard("owned terminal occurrence claim", async {
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
        let initial = player.inspect(&f.actor).await.unwrap().state.unwrap();
        let target_entry = initial.current.as_ref().unwrap().id;
        acquisition_started = Some(Instant::now());
        let command = player
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
        let _ = command;
        let selected = tokio::time::timeout(
            Duration::from_secs(3),
            state.wait_for(|snapshot| {
                snapshot.selection == 2
                    && snapshot.current_index == Some(1)
                    && snapshot.phase == PlayerPhase::Ready
                    && snapshot.playing
            }),
        )
        .await
        .map_err(|_| "terminal refusal did not reach the ready sentinel within the existing bound")?
        .map_err(|_| "the authorized state observer closed")?
        .clone();
        if selected
            .current
            .as_ref()
            .is_none_or(|entry| entry.recording_id != f.sentinel)
            || selected.last_failure.as_ref().is_none_or(|failure| {
                failure.entry.id != target_entry || failure.error != PlayerError::Playback
            })
        {
            return Err(
                "the sentinel was not selected after recording the target playback failure",
            );
        }
        let sentinel_entry = selected
            .current
            .as_ref()
            .map(|entry| entry.id)
            .ok_or("the queued sentinel occurrence identity was unavailable")?;
        if selected
            .current
            .as_ref()
            .is_none_or(|entry| entry.id != sentinel_entry)
        {
            return Err("queue advancement did not select the captured sentinel occurrence");
        }
        let progressed = *tokio::time::timeout(
            Duration::from_secs(3),
            position.wait_for(|observed| {
                observed.selection == 2
                    && observed.entry == Some(sentinel_entry)
                    && observed.position_ms > 400
            }),
        )
        .await
        .map_err(|_| "sentinel audio did not produce progressing native output")?
        .map_err(|_| "the authorized position observer closed")?;
        if progressed.position_ms <= 400 {
            return Err("sentinel playback did not advance beyond its saved offset");
        }
        let after_progress = player.inspect(&f.actor).await.unwrap().state.unwrap();
        if after_progress.selection != 2
            || after_progress.current_index != Some(1)
            || after_progress
                .current
                .as_ref()
                .is_none_or(|entry| entry.id != sentinel_entry || entry.recording_id != f.sentinel)
            || after_progress.phase != PlayerPhase::Ready
            || !after_progress.playing
            || after_progress.last_failure.as_ref().is_none_or(|failure| {
                failure.entry.id != target_entry || failure.error != PlayerError::Playback
            })
        {
            return Err("playback did not remain on the ready sentinel after one target failure");
        }
        capture.after(capture.mark(), 0.1);
        let presented = capture.recent(0.1);
        if capture::amplitude(&presented, 880.) <= 0.2
            || capture::amplitude(&presented, 440.) >= 0.02
        {
            return Err("the private monitor did not present only the queued sentinel audio");
        }
        let target_after = f
            .services
            .catalog
            .recordings_by_ids(&f.actor, &[f.recording])
            .await
            .unwrap();
        let sources_after = f
            .services
            .catalog
            .sources(&f.actor, &f.recording.to_string())
            .await
            .unwrap();
        let preferences_after = f
            .services
            .catalog
            .source_preferences(&f.actor)
            .await
            .unwrap();
        let target_file_after = std::fs::read(&target_path).unwrap();
        let sentinel_file_after = std::fs::read(&sentinel_path).unwrap();
        let target_mtime_after = std::fs::metadata(&target_path).unwrap().modified().unwrap();
        let sentinel_mtime_after = std::fs::metadata(&sentinel_path)
            .unwrap()
            .modified()
            .unwrap();
        let events = logs.events();
        let attempts = events
            .iter()
            .filter(|event| event["fields"]["event"] == "player_source_discovery_attempt_started")
            .collect::<Vec<_>>();
        if attempts.len() != 1
            || attempts[0]["fields"]["attempt"] != json!(1)
            || attempts[0]["fields"]["selection"] != json!(1)
        {
            return Err("the failed target must have exactly one logical discovery attempt");
        }
        let request_id = attempts[0]["fields"]["request_id"]
            .as_str()
            .ok_or("discovery did not publish a request identity")?;
        let request_id =
            Uuid::parse_str(request_id).map_err(|_| "discovery request identity was invalid")?;
        if request_id.is_nil() {
            return Err("discovery request identity must not be nil");
        }
        Ok((
            target_after,
            sources_after,
            preferences_after,
            target_file_after,
            sentinel_file_after,
            target_mtime_after,
            sentinel_mtime_after,
            request_id,
            sentinel_entry,
            f.order.lock().unwrap().clone(),
            f.recording.to_string(),
            acquisition_started.unwrap().elapsed(),
        ))
    })
    .catch_unwind()
    .await;

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
    let output = std::panic::catch_unwind(AssertUnwindSafe(|| {
        witness.as_ref().map(OutputWitness::finish)
    }));
    let output_joined = matches!(&output, Ok(None) | Ok(Some(Ok(_))))
        || matches!(&output, Ok(Some(Err(error))) if error != &WitnessError::ActiveWriters);
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

    let counts_before_replay = provider_counts(&f);
    let protocol_before_replay = f.protocol_clean();
    let order_after_player = f.order.lock().unwrap().clone();
    let provider_operations_before = [
        f.first.requests_for("v1/search").len(),
        f.second.requests_for("v1/search").len(),
        f.second.requests_for("v1/recordings/detail").len(),
        f.second.requests_for("v1/sources").len(),
        f.second.requests_for("v1/audio").len(),
    ];
    let private_values = f.private_values();
    let replay_value = match &behavior {
        Ok(Ok((_, _, _, _, _, _, _, request_id, _, _, recording_id, _))) => {
            AssertUnwindSafe(async {
                tokio::time::timeout(
                    Duration::from_secs(3),
                    f.services.catalog.resolve_source_until(
                        &f.actor,
                        recording_id,
                        ResolveSource {
                            request_id: *request_id,
                        },
                        std::future::pending(),
                    ),
                )
                .await
            })
            .catch_unwind()
            .await
            .ok()
            .and_then(Result::ok)
            .and_then(Result::ok)
            .and_then(|resolution| serde_json::to_value(resolution).ok())
        }
        _ => None,
    };
    let counts_after_replay = provider_counts(&f);
    let protocol_clean = f.protocol_clean();
    let order_after_replay = f.order.lock().unwrap().clone();
    let provider_operations_after = [
        f.first.requests_for("v1/search").len(),
        f.second.requests_for("v1/search").len(),
        f.second.requests_for("v1/recordings/detail").len(),
        f.second.requests_for("v1/sources").len(),
        f.second.requests_for("v1/audio").len(),
    ];
    let fixture_joined = AssertUnwindSafe(fixture::close(f))
        .catch_unwind()
        .await
        .is_ok_and(|joined| joined);
    let elapsed = started.elapsed();
    tracing::info!(
        event = "native_terminal_occurrence_cleanup",
        elapsed_ms = elapsed.as_millis() as u64,
        search_calls = searches.load(Ordering::SeqCst),
        owner_joined,
        player_dropped,
        witness_dropped,
        capture_dropped,
        output_joined,
        fixture_joined,
        "Owned terminal occurrence cleanup completed"
    );
    assert!(
        owner_joined
            && closing_dropped
            && player_dropped
            && output_joined
            && witness_dropped
            && capture_dropped
            && fixture_joined
            && protocol_before_replay
            && protocol_clean
    );
    assert!(
        matches!(silent, Ok(true)),
        "joined output must leave the monitor silent"
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
    let (
        target_after,
        sources_after,
        preferences_after,
        target_file_after,
        sentinel_file_after,
        target_mtime_after,
        sentinel_mtime_after,
        request_id,
        sentinel_entry,
        order,
        recording_id,
        acquisition_elapsed,
    ) = match behavior {
        Ok(Ok(result)) => result,
        Ok(Err(reason)) => panic!("{reason}"),
        Err(panic) => std::panic::resume_unwind(panic),
    };
    let replay =
        replay_value.expect("accepted provider refusal must replay within the existing bound");
    assert!(elapsed < Duration::from_secs(15));
    assert!(acquisition_elapsed < Duration::from_secs(15));
    assert_eq!(searches.load(Ordering::SeqCst), 1);
    assert_eq!(order, vec!['A', 'B']);
    assert_eq!(order_after_player, vec!['A', 'B']);
    assert_eq!(
        order_after_replay, order_after_player,
        "receipt replay must not call providers"
    );
    assert_eq!(
        counts_after_replay, counts_before_replay,
        "receipt replay must not call providers"
    );
    assert_eq!(provider_operations_before, [1, 1, 0, 0, 0]);
    assert_eq!(provider_operations_after, provider_operations_before);
    assert!(target_before.len() == 1 && target_after.len() == 1);
    assert_eq!(
        serde_json::to_value(&target_after[0]).unwrap(),
        serde_json::to_value(&target_before[0]).unwrap()
    );
    assert!(target_after[0].liked);
    assert_eq!(sources_after.len(), sources_before.len());
    assert_eq!(
        serde_json::to_value(&sources_after).unwrap(),
        serde_json::to_value(&sources_before).unwrap()
    );
    assert_eq!(preferences_after, preferences_before);
    assert_eq!(target_file_after, target_file_before);
    assert_eq!(sentinel_file_after, sentinel_file_before);
    assert_eq!(target_mtime_after, target_mtime_before);
    assert_eq!(sentinel_mtime_after, sentinel_mtime_before);
    assert_eq!(replay["outcome"], json!("provider_unavailable"));
    assert_eq!(
        replay["failure"],
        json!({"reason":"provider_refused","retryable":false})
    );
    assert_eq!(replay["request_id"], json!(request_id.to_string()));
    assert_eq!(replay["recording_id"], json!(recording_id));
    assert!(!sentinel_entry.is_nil());
    let events = logs.events();
    let attempts = events
        .iter()
        .filter(|event| event["fields"]["event"] == "player_source_discovery_attempt_finished")
        .collect::<Vec<_>>();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["fields"]["attempt"], json!(1));
    assert_eq!(
        attempts[0]["fields"]["reason"],
        json!("provider_unavailable")
    );
    assert_eq!(attempts[0]["fields"]["outcome"], json!("exhausted"));
    assert!(
        events
            .iter()
            .filter(|event| event["fields"]["operation"] == json!("v1/search"))
            .any(|event| event["fields"]["status"] == json!(422))
    );
}

fn provider_counts(f: &fixture::Fixture) -> [usize; 2] {
    [f.first.requests().len(), f.second.requests().len()]
}

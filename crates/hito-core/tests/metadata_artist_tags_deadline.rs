#![cfg(unix)]

use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::test_support::support;
#[path = "fixtures/metadata_tag_deadline.rs"]
mod tool;

use futures_util::FutureExt;
use hito_core::{Action, CoreConfig, HostDeviceId, MetadataHistoryQuery, Search};
use serde_json::Value;
use std::{panic::AssertUnwindSafe, time::Duration};
use tokio::{sync::watch, time::Instant};
use uuid::Uuid;

const PROBE_BUDGET: Duration = Duration::from_secs(4);
const EXPANDED_HOLD: Duration = Duration::from_millis(2500);
const SCHEDULING_TOLERANCE: Duration = Duration::from_secs(1);

#[tokio::test]
async fn fallback_shares_the_original_deadline_and_retires_both_owned_attempts() {
    let (directory, original, actor, library, music) = support::library("tag-deadline").await;
    let original_path = music.join("verified.wav");
    let original_bytes = support::write_wav(&original_path, 0);
    support::scan(&original, &actor, &library).await;
    let recording = original
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    support::synchronize(
        &original,
        &actor,
        vec![Action::RecordingTitleSet {
            recording_id: Uuid::parse_str(&recording.id).unwrap(),
            title: "Private deadline title canary".into(),
        }],
    )
    .await;
    let before = serde_json::to_value(
        original
            .catalog
            .recording_metadata(&actor, &recording.id, MetadataHistoryQuery::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let sources = original
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap();
    let source = sources[0].id.clone();
    let sources = serde_json::to_value(sources).unwrap();
    drop(original);
    let candidate_path = music.join("private-deadline-candidate-canary.wav");
    let candidate_bytes = support::write_wav(&candidate_path, 1);
    let tool = tool::Tool::new(directory.path());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("tag-deadline").unwrap(),
    );
    config.media_tools.ffprobe = tool.executable.clone();
    config.media_tools.timeout = PROBE_BUDGET;
    let hito = hito_core::open(config).await.unwrap();
    let logs = logs::CapturedLogs::default();
    tracing::subscriber::set_global_default(logs.subscriber()).unwrap();
    let job = hito
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    let (stop, mut stopped) = watch::channel(false);
    let storage = hito.storage.clone();
    let mut worker = tokio::spawn(async move {
        storage
            .run_next_job_until(async move {
                let stopping = *stopped.borrow();
                if !stopping {
                    let _ = stopped.changed().await;
                }
            })
            .await
    });
    let mut joined = false;
    let outcome = AssertUnwindSafe(async {
        let expanded = tool.started("expanded").await;
        let observed = Instant::now();
        expanded.assert_private_input(&candidate_bytes);
        tokio::time::sleep_until(observed + EXPANDED_HOLD).await;
        expanded.assert_private_input(&candidate_bytes);
        tool.release_expanded();
        let ordinary = tool.started("ordinary").await;
        ordinary.assert_private_input(&candidate_bytes);
        assert!(expanded.input == ordinary.input);
        assert!(ordinary.selector.ends_with(
            "format_tags=title,artist,album,album_artist,disc,track:\
             stream_tags=title,artist,album,album_artist,disc,track"
        ));
        assert!(!ordinary.selector.contains("MUSICBRAINZ"));
        let result =
            tokio::time::timeout_at(observed + PROBE_BUDGET + SCHEDULING_TOLERANCE, &mut worker)
                .await
                .expect("Fallback must not receive a fresh probe deadline");
        joined = true;
        assert!(result.unwrap().unwrap());
        assert!(observed.elapsed() >= PROBE_BUDGET - SCHEDULING_TOLERANCE);
        assert_eq!(tool.attempts(), ["expanded", "ordinary"]);
    })
    .catch_unwind()
    .await;

    stop.send_replace(true);
    if !joined
        && tokio::time::timeout(Duration::from_secs(5), &mut worker)
            .await
            .is_err()
    {
        tool.kill_owned();
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .expect("The cancelled fixture worker must retire")
            .unwrap()
            .unwrap();
    }
    let mut settlement =
        tokio::time::timeout(Duration::from_secs(5), hito.storage.finish_indexing()).await;
    if settlement.is_err() {
        tool.kill_owned();
        settlement =
            tokio::time::timeout(Duration::from_secs(5), hito.storage.finish_indexing()).await;
    }
    let retired = tool.retired().await;
    settlement
        .expect("The fixture must join private media cleanup")
        .unwrap();
    assert!(
        retired,
        "Both owned processes and their private input must retire"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }

    let job = hito.storage.job(&actor, &job.id).await.unwrap();
    assert_eq!(job.state, "complete");
    assert_eq!(job.progress, 1);
    assert!(job.error.is_none());
    let current = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(current.total, 1);
    assert_eq!(current.items[0].id, recording.id);
    assert!(current.items[0].available);
    assert_eq!(
        serde_json::to_value(hito.catalog.sources(&actor, &recording.id).await.unwrap()).unwrap(),
        sources
    );
    assert_eq!(
        serde_json::to_value(
            hito.catalog
                .recording_metadata(&actor, &recording.id, MetadataHistoryQuery::default())
                .await
                .unwrap()
        )
        .unwrap(),
        before
    );
    assert!(
        support::audio_bytes(hito.catalog.audio_file(&actor, &source).await.unwrap())
            == original_bytes
    );
    assert!(std::fs::read(&original_path).unwrap() == original_bytes);
    assert!(std::fs::read(&candidate_path).unwrap() == candidate_bytes);
    let output = logs.text();
    let events: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for reason in ["output_limit", "timeout"] {
        assert!(events.iter().any(|event| {
            event["fields"]["event"] == "media_command_failed"
                && event["fields"]["reason"] == reason
        }));
    }
    assert!(events.iter().any(|event| {
        event["fields"]["files_read"] == 1 && event["fields"]["files_skipped"] == 1
    }));
    for private in [
        "private-deadline-output-canary",
        "Private deadline title canary",
        "private-deadline-candidate-canary",
        directory.path().to_str().unwrap(),
    ] {
        assert!(
            !output.contains(private),
            "Private fixture data appeared in logs"
        );
    }
}

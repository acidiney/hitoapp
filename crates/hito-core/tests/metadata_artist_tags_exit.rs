#![cfg(target_os = "linux")]

use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::test_support::support;
#[path = "fixtures/metadata_tag_diagnostics.rs"]
mod tool;

use futures_util::FutureExt;
use hito_core::{CoreConfig, HostDeviceId, Search};
use serde_json::Value;
use std::{panic::AssertUnwindSafe, time::Duration};
use tokio::sync::watch;

#[tokio::test(flavor = "current_thread")]
async fn a_proven_natural_nonzero_exit_prevents_stdout_overflow_fallback() {
    let (directory, initial, actor, library, music) = support::library("tag-exit").await;
    drop(initial);
    let candidate = music.join("private-combined-exit-path-canary.wav");
    let bytes = support::write_wav(&candidate, 0);
    let tool = tool::Tool::new(directory.path(), tool::Completion::NonzeroExit);
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("tag-exit").unwrap(),
    );
    config.media_tools.ffprobe = tool.executable.clone();
    config.media_tools.timeout = Duration::from_secs(8);
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
        tool.ready().await;
        tool.drain_prefix().await;
        tool.complete_stdout_and_observe_exit_while_runtime_is_paused();
        let result = tokio::time::timeout(Duration::from_secs(5), &mut worker)
            .await
            .expect("The failed-exit attempt must finish within its original budget");
        joined = true;
        assert!(result.unwrap().unwrap());
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
            .expect("The owned fixture worker must retire")
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
    assert!(retired, "All owned attempts and private media must retire");
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }

    let output = logs.text();
    let events: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        events
            .iter()
            .any(|event| event["fields"]["event"] == "media_command_failed")
    );
    for private in [
        "private-combined-stdout-canary",
        "private-combined-stderr-canary",
        "private-combined-exit-path-canary",
        directory.path().to_str().unwrap(),
    ] {
        assert!(
            !output.contains(private),
            "Private tool data appeared in logs"
        );
    }
    assert!(
        !tool.ordinary_attempted(),
        "A proven natural nonzero exit must prevent ordinary metadata fallback"
    );
    assert!(events.iter().any(|event| {
        event["fields"]["event"] == "media_command_failed" && event["fields"]["reason"] == "exit"
    }));
    let job = hito.storage.job(&actor, &job.id).await.unwrap();
    assert_eq!(job.state, "complete");
    assert_eq!(job.progress, 0);
    assert!(job.error.is_none());
    assert_eq!(
        hito.catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert!(std::fs::read(candidate).unwrap() == bytes);
}

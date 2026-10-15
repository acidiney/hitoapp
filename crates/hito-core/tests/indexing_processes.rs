#![cfg(unix)]

#[path = "fixtures/media_tool.rs"]
mod media_tool;
mod support;

use hito_core::{CoreConfig, HostDeviceId};
use std::{path::Path, time::Duration};

async fn started(marker: &Path) -> (String, String) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(value) = std::fs::read_to_string(marker) {
                let mut lines = value.lines();
                if let (Some(pid), Some(input)) = (lines.next(), lines.next()) {
                    return (pid.to_owned(), input.to_owned());
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Media tool must start")
}

async fn stopped(pid: &str, input: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let running = std::process::Command::new("kill")
                .args(["-0", pid])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success();
            if !running && !Path::new(input).exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Media process and its private input must be released");
}

async fn exercise(cancel: bool) {
    let (dir, _original, actor, library, music) = support::library("server").await;
    support::write_wav(&music.join("track.wav"), 0);
    let marker = dir.path().join("process");
    let tool = media_tool::script(
        dir.path(),
        "ffprobe",
        &format!(
            "previous=''\nfor argument do\n  if [ \"$previous\" = '-i' ]; then input=$argument; fi\n  previous=$argument\ndone\nprintf '%s\\n%s\\n' \"$$\" \"$input\" > {}\nexec sleep 300",
            media_tool::quote(&marker),
        ),
    );
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffprobe = tool;
    config.media_tools.timeout = Duration::from_secs(1);
    let hito = hito_core::open(config).await.unwrap();
    let job = hito.indexer.enqueue_scan(&actor, &library).await.unwrap();
    let indexer = hito.indexer.clone();
    let worker = tokio::spawn(async move { indexer.run_next_job().await });
    let (pid, input) = started(&marker).await;
    assert!(Path::new(&input).exists());
    if cancel {
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
    } else {
        assert!(worker.await.unwrap().unwrap());
        assert_eq!(
            hito.indexer.job(&actor, &job.id).await.unwrap().state,
            "complete"
        );
    }
    stopped(&pid, &input).await;
}

#[tokio::test]
async fn a_timed_out_media_tool_is_reaped_and_releases_its_private_input() {
    exercise(false).await;
}

#[tokio::test]
async fn cancelling_a_scan_reaps_its_media_tool_and_releases_its_private_input() {
    exercise(true).await;
}

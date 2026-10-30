#![cfg(unix)]

use serde_json::Value;
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::timeout,
};

async fn assert_signal_shutdown(signal: &str) {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("shutdown-private-path.db");
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hito-server"))
        .env("HITO_DATABASE", &database)
        .env("HITO_MUSIC_ROOT", directory.path().join("music"))
        .env("HITO_WEB_DIR", web)
        .env("HITO_LISTEN", "127.0.0.1:0")
        .env("HITO_LOG_FORMAT", "json")
        .env("RUST_LOG", "hito_server=info,hito_core=info")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut error_lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let errors = tokio::spawn(async move {
        let mut collected = Vec::new();
        while let Some(line) = error_lines.next_line().await.unwrap() {
            collected.push(line);
        }
        collected
    });
    let mut logs = Vec::new();
    timeout(Duration::from_secs(10), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            let record: Value = serde_json::from_str(&line).unwrap();
            logs.push(line);
            if record["fields"]["event"] == "server_listening" {
                return;
            }
        }
        panic!("Server exited before becoming ready");
    })
    .await
    .expect("Server did not become ready in time");
    let remaining = tokio::spawn(async move {
        let mut collected = Vec::new();
        while let Some(line) = lines.next_line().await.unwrap() {
            collected.push(line);
        }
        collected
    });

    let signalled = Command::new("sh")
        .args(["-c", "kill -\"$1\" \"$2\"", "hito-shutdown-test", signal])
        .arg(child.id().unwrap().to_string())
        .status()
        .await
        .unwrap();
    assert!(signalled.success());
    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("Server did not finish shutdown in time")
        .unwrap();
    assert!(
        status.success(),
        "Server did not exit successfully after SIG{signal}: {status}"
    );
    logs.extend(remaining.await.unwrap());
    let output = logs.join("\n");
    let error_output = errors.await.unwrap().join("\n");
    let events = logs
        .iter()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    for expected in ["shutdown_requested", "worker_stopped", "server_stopped"] {
        assert!(
            events
                .iter()
                .any(|record| record["fields"]["event"] == expected),
            "Missing lifecycle event: {expected}"
        );
    }
    assert!(
        !output.contains("shutdown-private-path")
            && !error_output.contains("shutdown-private-path"),
        "Private database path appeared in logs"
    );
}

#[tokio::test]
async fn sigterm_stops_the_server_and_worker_cleanly() {
    assert_signal_shutdown("TERM").await;
}

#[tokio::test]
async fn sigint_stops_the_server_and_worker_cleanly() {
    assert_signal_shutdown("INT").await;
}

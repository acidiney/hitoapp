use super::boundary::{TITLE, TOKEN};
use serde_json::Value;
use std::{
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    task::JoinHandle,
};

const OUTPUT_LIMIT: usize = 8192;

pub(super) struct Output {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Output {
    fn redacted(&self, database: &Path) {
        for bytes in [&self.stdout, &self.stderr] {
            for canary in [
                TITLE,
                TOKEN,
                database.to_str().unwrap(),
                database.parent().unwrap().to_str().unwrap(),
                database.file_name().unwrap().to_str().unwrap(),
            ] {
                assert!(
                    !bytes
                        .windows(canary.len())
                        .any(|window| window == canary.as_bytes()),
                    "maintenance output exposed a private fixture canary"
                );
            }
        }
    }

    pub(super) fn assert_failure(&self, database: &Path) {
        self.redacted(database);
        assert!(
            !self.status.success(),
            "maintenance must reject this database boundary"
        );
        assert!(
            self.stdout.is_empty(),
            "a rejected command must not publish success output"
        );
        let value: Value = serde_json::from_slice(&self.stderr)
            .unwrap_or_else(|_| panic!("failure output must be one safe structured outcome"));
        let fields = value
            .as_object()
            .expect("failure output must be a structured object");
        assert!(
            fields.len() == 4
                && fields
                    .keys()
                    .all(|key| ["event", "outcome", "error_code", "elapsed_ms"]
                        .contains(&key.as_str())),
            "failure output must contain only safe outcome fields"
        );
        assert!(value["event"] == "database_migration_finished" && value["outcome"] == "failed");
        assert!(value["error_code"].as_str().is_some_and(|code| {
            [
                "invalid",
                "not_found",
                "conflict",
                "migration",
                "database",
                "internal",
            ]
            .contains(&code)
        }));
        assert!(value["elapsed_ms"].as_u64().is_some());
    }

    pub(super) fn assert_success(&self, database: &Path, previous: i64, current: i64, count: u64) {
        self.redacted(database);
        assert!(
            self.status.success(),
            "maintenance must accept this exact-version migration"
        );
        assert!(
            self.stderr.is_empty(),
            "successful maintenance must not publish failure output"
        );
        let value: Value = serde_json::from_slice(&self.stdout)
            .unwrap_or_else(|_| panic!("success output must be one safe structured outcome"));
        let fields = value
            .as_object()
            .expect("success output must be a structured object");
        assert!(
            fields.len() == 6
                && fields.keys().all(|key| [
                    "event",
                    "outcome",
                    "previous_version",
                    "current_version",
                    "applied_count",
                    "elapsed_ms"
                ]
                .contains(&key.as_str())),
            "success output must contain only safe outcome fields"
        );
        assert!(value["event"] == "database_migration_finished" && value["outcome"] == "complete");
        assert!(value["previous_version"].as_i64() == Some(previous));
        assert!(value["current_version"].as_i64() == Some(current));
        assert!(value["applied_count"].as_u64() == Some(count));
        assert!(value["elapsed_ms"].as_u64().is_some());
    }
}

pub(super) async fn run(database: &Path, expected: i64) -> Output {
    run_at(database, expected, None).await
}

pub(super) async fn run_at(database: &Path, expected: i64, directory: Option<&Path>) -> Output {
    let missing = database.parent().unwrap().join("unavailable");
    let argument = if directory.is_some() {
        database.file_name().unwrap()
    } else {
        database.as_os_str()
    };
    let mut command = Command::new(env!("CARGO_BIN_EXE_hito-server"));
    command
        .args(["migrate", "--database"])
        .arg(argument)
        .arg("--expect-version")
        .arg(expected.to_string())
        .env("HITO_DATABASE", database)
        .env("HITO_WEB_DIR", missing.join("web"))
        .env("HITO_MUSIC_ROOT", missing.join("music"))
        .env("HITO_FFMPEG", missing.join("ffmpeg"))
        .env("HITO_FFPROBE", missing.join("ffprobe"))
        .env_remove("HITO_STORAGE_MANIFEST")
        .env_remove("HITO_DEVICE_ID")
        .env("HITO_LISTEN", "127.0.0.1:0")
        .env("HITO_LOG_FORMAT", "json")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let mut child = command
        .spawn()
        .unwrap_or_else(|_| panic!("owned maintenance child must start"));
    let stdout = tokio::spawn(capture(child.stdout.take().unwrap()));
    let stderr = tokio::spawn(capture(child.stderr.take().unwrap()));
    let result = tokio::time::timeout(Duration::from_secs(20), child.wait()).await;
    if !matches!(&result, Ok(Ok(_))) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let stdout = joined(stdout).await;
    let stderr = joined(stderr).await;
    let status = result
        .ok()
        .and_then(Result::ok)
        .expect("owned maintenance child must finish within its deadline");
    Output {
        status,
        stdout,
        stderr,
    }
}

struct Captured {
    bytes: Vec<u8>,
    complete: bool,
    overflow: bool,
}

async fn capture(mut pipe: impl AsyncRead + Unpin + Send + 'static) -> Captured {
    let mut output = Captured {
        bytes: Vec::new(),
        complete: false,
        overflow: false,
    };
    let mut buffer = [0; 1024];
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) => {
                output.complete = true;
                break;
            }
            Ok(count) if output.bytes.len() + count <= OUTPUT_LIMIT => {
                output.bytes.extend_from_slice(&buffer[..count])
            }
            Ok(_) => output.overflow = true,
            Err(_) => break,
        }
    }
    output
}

async fn joined(mut task: JoinHandle<Captured>) -> Vec<u8> {
    let captured = match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
        Ok(result) => {
            result.unwrap_or_else(|_| panic!("owned command output reader must complete"))
        }
        Err(_) => {
            task.abort();
            let _ = task.await;
            panic!("owned command output reader exceeded its deadline");
        }
    };
    assert!(
        captured.complete && !captured.overflow,
        "maintenance output must complete within its byte bound"
    );
    captured.bytes
}

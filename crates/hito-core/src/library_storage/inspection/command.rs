use crate::{Error, Result};
use std::{ffi::OsString, path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    sync::{Semaphore, oneshot},
};
use tracing::Instrument;

#[derive(Clone)]
pub(super) struct Commands(Arc<Semaphore>);

type Outcome<T> = std::result::Result<T, &'static str>;

impl Commands {
    pub(super) fn new() -> Self {
        Self(Arc::new(Semaphore::new(2)))
    }

    pub(super) async fn run(
        &self,
        program: &Path,
        arguments: Vec<OsString>,
        limit: usize,
        timeout: Duration,
        media_guard: Option<Arc<tempfile::TempDir>>,
    ) -> Result<Vec<u8>> {
        let program = program.to_owned();
        let slots = self.0.clone();
        let (mut sender, receiver) = oneshot::channel();
        let task = async move {
            let permit = tokio::select! {
                permit = slots.acquire_owned() => permit.ok(),
                _ = sender.closed() => return,
            };
            let Some(_permit) = permit else {
                return;
            };
            if sender.is_closed() {
                return;
            }
            let child = Command::new(program)
                .args(arguments)
                .env_remove("FFREPORT")
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .stderr(Stdio::piped())
                .stdout(Stdio::piped())
                .kill_on_drop(true)
                .spawn();
            let result = match child {
                Ok(mut child) => match supervise(&mut child, limit, timeout, &mut sender).await {
                    Ok(output) => Ok(output),
                    Err(reason) => {
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        tracing::debug!(
                            event = "media_command_failed",
                            reason,
                            "Media command stopped"
                        );
                        Err(Error::Internal("Media inspection could not finish".into()))
                    }
                },
                Err(_) => {
                    tracing::warn!(
                        event = "media_tool_unavailable",
                        "Media inspection tool unavailable"
                    );
                    Err(Error::Internal(
                        "Media inspection tool is unavailable".into(),
                    ))
                }
            };
            drop(media_guard);
            let _ = sender.send(result);
        };
        tokio::spawn(task.instrument(tracing::Span::current()));
        receiver
            .await
            .map_err(|_| Error::Internal("Media inspection stopped".into()))?
    }
}

async fn supervise(
    child: &mut Child,
    limit: usize,
    timeout: Duration,
    sender: &mut oneshot::Sender<Result<Vec<u8>>>,
) -> Outcome<Vec<u8>> {
    let stdout = child.stdout.take().ok_or("pipe")?;
    let stderr = child.stderr.take().ok_or("pipe")?;
    let operation = async {
        let (output, ()) =
            tokio::try_join!(read_output(stdout, limit), reject_diagnostics(stderr))?;
        let status = child.wait().await.map_err(|_| "wait")?;
        if !status.success() {
            return Err("exit");
        }
        Ok(output)
    };
    tokio::select! {
        result = tokio::time::timeout(timeout, operation) => result.unwrap_or(Err("timeout")),
        _ = sender.closed() => Err("cancelled"),
    }
}

async fn read_output(stream: impl AsyncRead + Unpin, limit: usize) -> Outcome<Vec<u8>> {
    let mut output = Vec::new();
    stream
        .take(limit as u64 + 1)
        .read_to_end(&mut output)
        .await
        .map_err(|_| "read")?;
    if output.len() > limit {
        return Err("output_limit");
    }
    Ok(output)
}

async fn reject_diagnostics(mut stream: impl AsyncRead + Unpin) -> Outcome<()> {
    let mut byte = [0];
    if stream.read(&mut byte).await.map_err(|_| "read")? > 0 {
        return Err("diagnostic");
    }
    Ok(())
}
